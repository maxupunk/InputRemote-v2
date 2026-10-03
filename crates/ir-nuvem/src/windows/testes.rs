//! A Cloud Files API de verdade, numa pasta temporária deste disco.
//!
//! Marcados `#[ignore]`: registram uma raiz de sincronia no HKCU do usuário que roda o teste e
//! mexem no Explorer dele por alguns segundos. Rodam à mão, na máquina de desenvolvimento:
//! `cargo test -p ir-nuvem -- --ignored`.

use std::sync::{Arc, Mutex};

use super::*;
use crate::Pedido;

/// O conteúdo de referência: cada byte é a posição dele, para um trecho trocado aparecer.
fn byte(posicao: u64) -> u8 {
    u8::try_from(posicao % 251).unwrap_or(0)
}

struct Raiz {
    caminho: std::path::PathBuf,
    id: String,
}

impl Raiz {
    fn nova(nome: &str) -> Self {
        let caminho = std::env::temp_dir().join(format!("ir-nuvem-{nome}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&caminho);
        std::fs::create_dir_all(&caminho).unwrap();
        let id = crate::id_da_raiz("teste", &format!("{nome}{}", std::process::id()));
        Self { caminho, id }
    }
}

impl Drop for Raiz {
    fn drop(&mut self) {
        desregistrar(&self.caminho, &self.id);
        let _ = std::fs::remove_dir_all(&self.caminho);
    }
}

/// Conecta à raiz respondendo cada pedido com o conteúdo de referência, de outra thread — como o
/// ajudante faz quando os bytes chegam da rede —, e anota os pedidos.
fn servir_o_conteudo(raiz: &Raiz, pedidos: &Arc<Mutex<Vec<Pedido>>>) -> Conexao {
    let anotados = Arc::clone(pedidos);
    conectar(&raiz.caminho, move |pedido| {
        anotados.lock().unwrap().push(pedido.clone());
        if let Pedido::Buscar(busca) = pedido {
            std::thread::spawn(move || {
                let dados: Vec<u8> = (busca.offset..busca.offset + busca.tamanho)
                    .map(byte)
                    .collect();
                entregar(busca.conexao, busca.transferencia, busca.offset, &dados).unwrap();
            });
        }
    })
    .unwrap()
}

/// Lê um arquivo por outro processo: o próprio provedor não hidrata (`BLOCK_SELF_IMPLICIT_HYDRATION`).
fn ler_de_fora(caminho: &std::path::Path) -> Vec<u8> {
    let copia = caminho.with_extension("copia");
    let status = std::process::Command::new("cmd")
        .args(["/c", "copy", "/b"])
        .arg(caminho)
        .arg(&copia)
        .output()
        .unwrap();
    assert!(
        status.status.success(),
        "{}",
        String::from_utf8_lossy(&status.stdout)
    );
    let lido = std::fs::read(&copia).unwrap();
    let _ = std::fs::remove_file(copia);
    lido
}

#[test]
#[ignore = "registra uma raiz de sincronia no Windows do usuário"]
fn um_marcador_e_lido_por_outro_programa_e_o_provedor_entrega_os_bytes() {
    let raiz = Raiz::nova("leitura");
    assert!(suportado(&raiz.caminho), "o disco temporário é NTFS");
    registrar(
        &raiz.caminho,
        &raiz.id,
        "InputRemote — teste",
        "%SystemRoot%\\system32\\imageres.dll,-3",
    )
    .unwrap();
    let pedidos = Arc::new(Mutex::new(Vec::new()));
    let conexao = servir_o_conteudo(&raiz, &pedidos);

    let tamanho = 300_000u64;
    let arquivo = raiz.caminho.join("relatório.bin");
    criar_marcador(&arquivo, tamanho, 1_790_962_200_000_000_000, false).unwrap();
    let dados = std::fs::symlink_metadata(&arquivo).unwrap();
    let antes = situacao(&dados);
    assert!(antes.sem_conteudo, "o marcador nasce sem conteúdo");
    // O marcador é ponto de nova análise, mas não "substituto de nome": a varredura o vê como
    // arquivo, e não o pula como pularia uma ligação simbólica.
    assert!(!dados.file_type().is_symlink() && dados.is_file());
    assert_eq!(
        std::fs::metadata(&arquivo).unwrap().len(),
        tamanho,
        "mas com o tamanho certo"
    );

    let lido = ler_de_fora(&arquivo);
    let esperado: Vec<u8> = (0..tamanho).map(byte).collect();
    assert_eq!(lido, esperado, "os bytes vieram do provedor");
    let depois = situacao(&std::fs::symlink_metadata(&arquivo).unwrap());
    assert!(!depois.sem_conteudo, "hidratado");
    {
        let feitos = pedidos.lock().unwrap();
        let busca = feitos.iter().find_map(|p| match p {
            Pedido::Buscar(b) => Some(b.clone()),
            Pedido::Cancelar { .. } => None,
        });
        assert_eq!(busca.map(|b| b.caminho), Some("relatório.bin".to_owned()));
    }

    desidratar(&arquivo).unwrap();
    assert!(
        situacao(&std::fs::symlink_metadata(&arquivo).unwrap()).sem_conteudo,
        "liberar espaço"
    );

    let pasta = raiz.caminho.join("sub");
    criar_marcador(&pasta, 0, 1_790_962_200_000_000_000, true).unwrap();
    assert!(pasta.is_dir());
    let novo = pasta.join("nascido-aqui.txt");
    std::fs::write(&novo, b"local").unwrap();
    marcar_em_dia(&novo).unwrap();
    reverter(&novo).unwrap();
    drop(conexao);
}

#[test]
#[ignore = "registra uma raiz de sincronia no Windows do usuário"]
fn sem_o_provedor_responder_bem_quem_abre_recebe_erro_e_nao_trava() {
    let raiz = Raiz::nova("falha");
    registrar(
        &raiz.caminho,
        &raiz.id,
        "InputRemote — teste",
        "%SystemRoot%\\system32\\imageres.dll,-3",
    )
    .unwrap();
    let conexao = conectar(&raiz.caminho, |pedido| {
        if let Pedido::Buscar(busca) = pedido {
            let r = falhar(
                busca.conexao,
                busca.transferencia,
                (busca.offset, busca.tamanho),
            );
            eprintln!("falhar({}, {}): {r:?}", busca.offset, busca.tamanho);
        }
    })
    .unwrap();
    let arquivo = raiz.caminho.join("longe.txt");
    criar_marcador(&arquivo, 10, 1_790_962_200_000_000_000, false).unwrap();
    let comando = format!(
        "try {{ [IO.File]::ReadAllBytes('{}') | Out-Null; exit 0 }} catch {{ $_.Exception.Message; exit 1 }}",
        arquivo.display()
    );
    let saida = std::process::Command::new("powershell")
        .args(["-NoProfile", "-Command", &comando])
        .output()
        .unwrap();
    let mensagem = String::from_utf8_lossy(&saida.stdout).into_owned();
    eprintln!("quem leu ouviu: {mensagem}");
    assert!(
        !saida.status.success(),
        "a leitura falha com o motivo, em vez de esperar para sempre"
    );
    drop(conexao);
}

#[test]
#[ignore = "registra uma raiz de sincronia no Windows do usuário"]
fn na_pasta_do_usuario_a_raiz_entra_no_painel_do_explorer() {
    let perfil = std::path::PathBuf::from(std::env::var_os("USERPROFILE").unwrap());
    let caminho = perfil.join(format!("ir-nuvem-painel-{}", std::process::id()));
    std::fs::create_dir_all(&caminho).unwrap();
    let raiz = Raiz {
        caminho,
        id: crate::id_da_raiz("teste", &format!("painel{}", std::process::id())),
    };
    let antes = painel();
    registrar(
        &raiz.caminho,
        &raiz.id,
        "InputRemote - teste do painel",
        r"%SystemRoot%\system32\imageres.dll,-3",
    )
    .unwrap();
    let info =
        windows::Storage::Provider::StorageProviderSyncRootManager::GetSyncRootInformationForId(
            &windows::core::HSTRING::from(raiz.id.as_str()),
        );
    assert!(info.is_ok(), "{:?}", info.err().map(|e| e.message()));
    let depois = painel();
    let novos: Vec<&String> = depois.iter().filter(|l| !antes.contains(l)).collect();
    eprintln!("entradas novas no painel: {novos:?}");
    assert!(
        !novos.is_empty(),
        "o Windows pôs a raiz no painel lateral sozinho"
    );
}

/// As entradas do painel lateral do Explorer deste usuário.
fn painel() -> Vec<String> {
    let saida = std::process::Command::new("reg")
        .args([
            "query",
            r"HKCU\Software\Microsoft\Windows\CurrentVersion\Explorer\Desktop\NameSpace",
        ])
        .output()
        .unwrap();
    String::from_utf8_lossy(&saida.stdout)
        .lines()
        .map(str::to_owned)
        .collect()
}

#[test]
#[ignore = "lê as raízes de sincronia do Windows do usuário"]
fn as_raizes_registradas_sao_lidas_do_registro() {
    let raiz = Raiz::nova("lista");
    registrar(
        &raiz.caminho,
        &raiz.id,
        "InputRemote - teste",
        r"%SystemRoot%\system32\imageres.dll,-3",
    )
    .unwrap();
    let ids = super::registro::registradas_para_teste();
    assert!(ids.contains(&raiz.id), "{ids:?}");
    let prefixo = raiz.id.clone();
    assert_eq!(
        limpar_orfas(&prefixo, std::slice::from_ref(&raiz.id)),
        0,
        "a que existe fica"
    );
    assert_eq!(limpar_orfas(&prefixo, &[]), 1, "sem pasta, sai");
    assert!(!super::registro::registradas_para_teste().contains(&raiz.id));
}
