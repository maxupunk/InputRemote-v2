//! A bancada dos testes de dois ajudantes: cada um com o seu disco numa pasta temporária, ligados
//! um ao outro sem serviço no meio.

#![allow(
    dead_code,
    unreachable_pub,
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::path::{Path, PathBuf};

use ir_ipc::pastas::{ComandoDePasta, PapelDaPasta, SituacaoDaPasta};
use ir_proto::message::FolderMessage;
use ir_sincronia::{Lugar, Pastas};

pub struct Lado {
    pub pastas: Pastas,
    pub raiz_padrao: PathBuf,
    pub fila: Vec<FolderMessage>,
}

pub fn lado(base: &Path, nome: &str) -> Lado {
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

pub struct Bancada {
    pub base: PathBuf,
    pub a: Lado,
    pub b: Lado,
    pub ligado: bool,
    /// Quantos blocos de arquivo B (a réplica) mandou a A.
    pub blocos_de_b: usize,
    /// Quantos trechos de arquivo A (a origem) mandou a B.
    pub trechos_de_a: usize,
}

impl Bancada {
    pub fn nova(nome: &str) -> Self {
        let base = std::env::temp_dir().join(format!("ir-dois-{nome}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let mut bancada = Self {
            a: lado(&base, "desktop"),
            b: lado(&base, "notebook"),
            base,
            ligado: false,
            blocos_de_b: 0,
            trechos_de_a: 0,
        };
        bancada.ligar();
        bancada
    }

    pub fn ligar(&mut self) {
        self.ligado = true;
        self.a
            .pastas
            .enlace(true, true, "NOTEBOOK".into(), &mut self.a.fila);
        self.b
            .pastas
            .enlace(true, true, "DESKTOP".into(), &mut self.b.fila);
        self.rodar();
    }

    pub fn desligar(&mut self) {
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
    pub fn rodar(&mut self) {
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
            self.trechos_de_a += de_a
                .iter()
                .filter(|m| matches!(m, FolderMessage::Range { .. }))
                .count();
            self.blocos_de_b += de_b
                .iter()
                .filter(|m| matches!(m, FolderMessage::UploadBlock { .. }))
                .count();
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
    pub fn compartilhar(&mut self, arquivos: &[(&str, &[u8])]) -> (PathBuf, PathBuf) {
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

pub fn escrever(caminho: &Path, dados: &[u8]) {
    std::fs::create_dir_all(caminho.parent().unwrap()).unwrap();
    std::fs::write(caminho, dados).unwrap();
}

/// A árvore que o usuário vê: caminhos relativos e conteúdos, sem a pasta de controle.
pub fn arvore(raiz: &Path) -> Vec<(String, Option<Vec<u8>>)> {
    let mut saida = Vec::new();
    descer(raiz, raiz, &mut saida);
    saida.sort();
    saida
}

pub fn descer(raiz: &Path, dir: &Path, saida: &mut Vec<(String, Option<Vec<u8>>)>) {
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
