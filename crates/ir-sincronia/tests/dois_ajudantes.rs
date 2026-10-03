//! Dois ajudantes de verdade, cada um com o seu disco numa pasta temporária, ligados um ao outro sem
//! serviço no meio: o que um manda ao par é entregue ao outro. É o caminho inteiro do disco — montar
//! ao lado, conferir o resumo, publicar, a lixeira — com o motor de verdade decidindo.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::path::{Path, PathBuf};

use ir_ipc::pastas::{ComandoDePasta, PapelDaPasta, SituacaoDaPasta};
use ir_proto::message::FolderMessage;
use ir_sincronia::{Lugar, Pastas};

struct Lado {
    pastas: Pastas,
    raiz_padrao: PathBuf,
    fila: Vec<FolderMessage>,
}

fn lado(base: &Path, nome: &str) -> Lado {
    let lugar = Lugar {
        estado: base.join(nome).join("estado"),
        raiz_padrao: base.join(nome).join("InputRemote"),
        maquina: nome.to_uppercase(),
        fuso_s: 0,
    };
    std::fs::create_dir_all(&lugar.raiz_padrao).unwrap();
    Lado {
        raiz_padrao: lugar.raiz_padrao.clone(),
        pastas: Pastas::abrir(lugar),
        fila: Vec::new(),
    }
}

struct Bancada {
    base: PathBuf,
    a: Lado,
    b: Lado,
    ligado: bool,
}

impl Bancada {
    fn nova(nome: &str) -> Self {
        let base = std::env::temp_dir().join(format!("ir-dois-{nome}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let mut bancada = Self {
            a: lado(&base, "desktop"),
            b: lado(&base, "notebook"),
            base,
            ligado: false,
        };
        bancada.ligar();
        bancada
    }

    fn ligar(&mut self) {
        self.ligado = true;
        self.a
            .pastas
            .enlace(true, true, "NOTEBOOK".into(), &mut self.a.fila);
        self.b
            .pastas
            .enlace(true, true, "DESKTOP".into(), &mut self.b.fila);
        self.rodar();
    }

    fn desligar(&mut self) {
        self.ligado = false;
        self.a
            .pastas
            .enlace(false, true, "NOTEBOOK".into(), &mut self.a.fila);
        self.b
            .pastas
            .enlace(false, true, "DESKTOP".into(), &mut self.b.fila);
        self.a.fila.clear();
        self.b.fila.clear();
    }

    /// Varre os dois discos e entrega as mensagens até não haver mais nada a dizer.
    fn rodar(&mut self) {
        for _ in 0..10_000 {
            self.a.pastas.varrer(None, &mut self.a.fila);
            self.b.pastas.varrer(None, &mut self.b.fila);
            self.a.pastas.andar(&mut self.a.fila);
            self.b.pastas.andar(&mut self.b.fila);
            if !self.ligado {
                self.a.fila.clear();
                self.b.fila.clear();
                return;
            }
            let de_a = std::mem::take(&mut self.a.fila);
            let de_b = std::mem::take(&mut self.b.fila);
            if de_a.is_empty() && de_b.is_empty() {
                return;
            }
            for mensagem in de_a {
                self.b.pastas.do_par(mensagem, &mut self.b.fila);
            }
            for mensagem in de_b {
                self.a.pastas.do_par(mensagem, &mut self.a.fila);
            }
        }
        panic!("a conversa não parou");
    }

    /// A compartilha uma pasta com estes arquivos; B aceita. Devolve as duas raízes.
    fn compartilhar(&mut self, arquivos: &[(&str, &[u8])]) -> (PathBuf, PathBuf) {
        let raiz_a = self
            .base
            .join("desktop")
            .join("Documentos")
            .join("Projetos");
        for (caminho, dados) in arquivos {
            escrever(&raiz_a.join(caminho), dados);
        }
        std::fs::create_dir_all(&raiz_a).unwrap();
        let pedido = ComandoDePasta::Compartilhar {
            caminho: raiz_a.to_string_lossy().into_owned(),
        };
        self.a.pastas.comando(pedido, &mut self.a.fila).unwrap();
        self.rodar();
        let oferta = self
            .b
            .pastas
            .resumo()
            .into_iter()
            .next()
            .expect("a oferta chegou");
        assert_eq!(oferta.situacao, SituacaoDaPasta::Oferecida);
        assert_eq!(oferta.papel, PapelDaPasta::Recebida);
        self.b
            .pastas
            .comando(ComandoDePasta::Aceitar(oferta.id), &mut self.b.fila)
            .unwrap();
        self.rodar();
        let raiz_b = self.b.raiz_padrao.join("Projetos");
        (raiz_a, raiz_b)
    }
}

impl Drop for Bancada {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.base);
    }
}

fn escrever(caminho: &Path, dados: &[u8]) {
    std::fs::create_dir_all(caminho.parent().unwrap()).unwrap();
    std::fs::write(caminho, dados).unwrap();
}

/// A árvore que o usuário vê: caminhos relativos e conteúdos, sem a pasta de controle.
fn arvore(raiz: &Path) -> Vec<(String, Option<Vec<u8>>)> {
    let mut saida = Vec::new();
    descer(raiz, raiz, &mut saida);
    saida.sort();
    saida
}

fn descer(raiz: &Path, dir: &Path, saida: &mut Vec<(String, Option<Vec<u8>>)>) {
    for entrada in std::fs::read_dir(dir).unwrap().filter_map(Result::ok) {
        if entrada.file_name() == ".inputremote" {
            continue;
        }
        let caminho = entrada.path();
        let relativo = caminho
            .strip_prefix(raiz)
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        if caminho.is_dir() {
            saida.push((relativo, None));
            descer(raiz, &caminho, saida);
        } else {
            saida.push((relativo, Some(std::fs::read(&caminho).unwrap())));
        }
    }
}

#[test]
fn aceitar_traz_a_pasta_inteira_e_as_edicoes_vao_e_voltam() {
    let mut b = Bancada::nova("ida-e-volta");
    let (raiz_a, raiz_b) = b.compartilhar(&[
        ("relatório/março.xlsx", b"planilha"),
        ("notas.txt", b"oi"),
        ("vazio.bin", b""),
    ]);
    assert_eq!(arvore(&raiz_a), arvore(&raiz_b));
    assert_eq!(b.b.pastas.resumo()[0].situacao, SituacaoDaPasta::EmDia);

    escrever(&raiz_b.join("notas.txt"), b"editado no notebook");
    b.rodar();
    assert_eq!(
        std::fs::read(raiz_a.join("notas.txt")).unwrap(),
        b"editado no notebook"
    );

    escrever(
        &raiz_a.join("novo/fundo/grande.bin"),
        &vec![7u8; 9 * 1024 * 1024],
    );
    b.rodar();
    assert_eq!(
        arvore(&raiz_a),
        arvore(&raiz_b),
        "um arquivo de vários trechos e créditos"
    );
}

#[test]
fn offline_editado_nos_dois_lados_guarda_as_duas_versoes() {
    let mut b = Bancada::nova("conflito");
    let (raiz_a, raiz_b) = b.compartilhar(&[("proposta.docx", b"v1")]);
    b.desligar();
    escrever(&raiz_a.join("proposta.docx"), b"v2 do desktop");
    std::thread::sleep(std::time::Duration::from_millis(20));
    escrever(
        &raiz_b.join("proposta.docx"),
        b"v2 do notebook, mais recente",
    );
    b.rodar();
    b.ligar();
    let arvore_a = arvore(&raiz_a);
    assert_eq!(arvore_a, arvore(&raiz_b));
    assert_eq!(arvore_a.len(), 2, "{arvore_a:?}");
    let conteudos: Vec<&[u8]> = arvore_a.iter().filter_map(|(_, d)| d.as_deref()).collect();
    assert!(conteudos.contains(&b"v2 do desktop".as_slice()));
    assert!(conteudos.contains(&b"v2 do notebook, mais recente".as_slice()));
    assert_eq!(
        b.a.pastas.resumo()[0].conflitos,
        1,
        "a janela conta o conflito"
    );
}

#[test]
fn apagar_vai_para_a_lixeira_e_parar_deixa_os_arquivos() {
    let mut b = Bancada::nova("lixeira");
    let (raiz_a, raiz_b) = b.compartilhar(&[("a.txt", b"a"), ("d/b.txt", b"b")]);
    std::fs::remove_dir_all(raiz_b.join("d")).unwrap();
    b.rodar();
    assert!(!raiz_a.join("d").exists());
    let lixeira = raiz_a.join(".inputremote").join("lixeira");
    assert!(lixeira.exists(), "o que saiu foi para a lixeira da pasta");

    let id = b.a.pastas.resumo()[0].id;
    b.a.pastas
        .comando(ComandoDePasta::Parar(id), &mut b.a.fila)
        .unwrap();
    b.rodar();
    assert!(b.a.pastas.resumo().is_empty() && b.b.pastas.resumo().is_empty());
    assert_eq!(
        std::fs::read(raiz_b.join("a.txt")).unwrap(),
        b"a",
        "os arquivos ficam"
    );
}

#[test]
fn compartilhar_dentro_de_outra_pasta_compartilhada_e_recusado_com_o_motivo() {
    let mut b = Bancada::nova("sobreposta");
    let (raiz_a, _) = b.compartilhar(&[("x/y.txt", b"y")]);
    let pedido = ComandoDePasta::Compartilhar {
        caminho: raiz_a.join("x").to_string_lossy().into_owned(),
    };
    let erro = b.a.pastas.comando(pedido, &mut b.a.fila).unwrap_err();
    assert!(erro.contains("já é compartilhada"), "{erro}");
}

#[test]
fn reiniciar_o_ajudante_continua_de_onde_parou() {
    let mut b = Bancada::nova("reinicio");
    let (raiz_a, raiz_b) = b.compartilhar(&[("a.txt", b"a")]);
    b.desligar();
    escrever(&raiz_b.join("offline.txt"), b"feito offline");
    b.rodar();
    // O ajudante do notebook reinicia: tudo o que ele sabe sai do índice guardado.
    let lugar = Lugar {
        estado: b.base.join("notebook").join("estado"),
        raiz_padrao: b.b.raiz_padrao.clone(),
        maquina: "NOTEBOOK".into(),
        fuso_s: 0,
    };
    b.b.pastas = Pastas::abrir(lugar);
    b.ligar();
    assert_eq!(
        std::fs::read(raiz_a.join("offline.txt")).unwrap(),
        b"feito offline"
    );
    assert_eq!(arvore(&raiz_a), arvore(&raiz_b));
}

#[test]
fn enquanto_a_leva_de_mudancas_nao_termina_a_pasta_nao_diz_em_dia() {
    let mut b = Bancada::nova("leva-em-partes");
    let (raiz_a, _) = b.compartilhar(&[("a.txt", b"1")]);
    // Subpastas vazias, que não baixam nada — como os marcadores da réplica sob demanda —, com nomes
    // longos o bastante para a leva não caber numa mensagem só.
    let longo = "n".repeat(120);
    for i in 0..800 {
        std::fs::create_dir_all(raiz_a.join(format!("muitas/{i:04}-{longo}"))).unwrap();
    }
    b.a.pastas.varrer(None, &mut b.a.fila);
    let leva: Vec<FolderMessage> = std::mem::take(&mut b.a.fila)
        .into_iter()
        .filter(|m| matches!(m, FolderMessage::Changes { .. }))
        .collect();
    assert!(
        leva.len() > 1,
        "a leva tem de vir em partes para o teste valer"
    );
    for (i, mensagem) in leva.into_iter().enumerate() {
        if i == 1 {
            assert_eq!(
                b.b.pastas.resumo()[0].situacao,
                SituacaoDaPasta::Sincronizando,
                "no meio da leva, a pasta não está em dia"
            );
        }
        b.b.pastas.do_par(mensagem, &mut b.b.fila);
    }
}
