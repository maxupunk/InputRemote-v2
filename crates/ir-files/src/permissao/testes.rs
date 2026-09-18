//! A regra de quem pode ler, sem disco — e, no Linux, com um descritor de verdade.

use super::*;

const MAXUEL: u32 = 1000;
const ROOT: u32 = 0;

fn arquivo(uid: u32, modo: u32) -> Dono {
    Dono {
        uid,
        modo,
        pasta: false,
    }
}

fn pasta(uid: u32, modo: u32) -> Dono {
    Dono {
        uid,
        modo,
        pasta: true,
    }
}

const USUARIO: Leitor = Leitor::Usuario { uid: MAXUEL };

#[test]
fn quem_conectou_decide_o_leitor() {
    // Root e o próprio dono do serviço têm a autoridade dele.
    assert_eq!(Leitor::do_chamador(Some(ROOT), ROOT, true), Leitor::Proprio);
    assert_eq!(
        Leitor::do_chamador(Some(MAXUEL), MAXUEL, false),
        Leitor::Proprio
    );
    // O usuário da sessão falando com o serviço root: só o que ele leria.
    assert_eq!(Leitor::do_chamador(Some(MAXUEL), ROOT, true), USUARIO);
    // O serviço do Windows como SYSTEM, sem saber quem é: nada.
    assert_eq!(Leitor::do_chamador(None, ROOT, true), Leitor::Desconhecido);
    // Rodando à mão como o próprio usuário, sem saber quem é: não há fronteira a proteger.
    assert_eq!(Leitor::do_chamador(None, ROOT, false), Leitor::Proprio);
}

#[test]
fn o_shadow_nao_passa() {
    // O caso que motivou o módulo. `/etc/shadow` é `0000 root` no Fedora.
    assert!(!permite(&USUARIO, arquivo(ROOT, 0o000), Uso::Ler));
    assert!(!permite(&USUARIO, arquivo(ROOT, 0o640), Uso::Ler));
}

#[test]
fn o_arquivo_do_proprio_usuario_passa_qualquer_que_seja_o_modo() {
    assert!(permite(&USUARIO, arquivo(MAXUEL, 0o600), Uso::Ler));
    assert!(permite(&USUARIO, arquivo(MAXUEL, 0o000), Uso::Ler));
}

#[test]
fn o_que_qualquer_um_le_passa() {
    // `/etc/hostname` é `0644 root`: o usuário leria sozinho, então pode mandar.
    assert!(permite(&USUARIO, arquivo(ROOT, 0o644), Uso::Ler));
}

#[test]
fn legivel_so_por_grupo_e_recusado() {
    // Conservador de propósito: o usuário pode até estar no grupo, mas confirmar isso de forma
    // confiável não vale o risco. Ele copia para a própria pasta antes.
    assert!(!permite(&USUARIO, arquivo(ROOT, 0o640), Uso::Ler));
}

#[test]
fn listar_uma_pasta_alheia_exige_ler_e_atravessar() {
    assert!(permite(&USUARIO, pasta(ROOT, 0o755), Uso::Ler));
    assert!(!permite(&USUARIO, pasta(ROOT, 0o754), Uso::Ler), "sem x");
    assert!(!permite(&USUARIO, pasta(ROOT, 0o751), Uso::Ler), "sem r");
}

#[test]
fn um_arquivo_aberto_a_todos_dentro_de_uma_pasta_fechada_nao_e_alcancavel() {
    // `/root/segredo.txt` com `0644`: o bit do arquivo diz "qualquer um lê", mas ninguém além de
    // root chega até ele. A travessia de `/root` (`0550`) é o que decide.
    assert!(permite(&USUARIO, arquivo(ROOT, 0o644), Uso::Ler));
    assert!(!permite(&USUARIO, pasta(ROOT, 0o550), Uso::Atravessar));
}

#[test]
fn quem_tem_a_autoridade_do_servico_passa_sempre() {
    assert!(permite(&Leitor::Proprio, arquivo(ROOT, 0o000), Uso::Ler));
}

#[test]
fn quem_nao_se_sabe_quem_e_nao_passa_nunca() {
    // O serviço do Windows como SYSTEM, sem personificação ainda: nem o arquivo mais aberto.
    assert!(!permite(
        &Leitor::Desconhecido,
        arquivo(ROOT, 0o777),
        Uso::Ler
    ));
    assert!(!permite(
        &Leitor::Desconhecido,
        arquivo(MAXUEL, 0o777),
        Uso::Ler
    ));
    assert!(conferir_leitor(&Leitor::Desconhecido, Path::new("x")).is_err());
}

#[cfg(target_os = "linux")]
#[test]
fn o_arquivo_aberto_e_conferido_pelo_descritor() {
    // `/etc/shadow` não abre como usuário comum, então o teste usa um arquivo que abre e que
    // pertence a root: o `/etc/hostname`, que é `0644`. Um `uid` que não é o dono lê porque o
    // bit de outros deixa — e o mesmo arquivo com um leitor desconhecido é recusado.
    let caminho = Path::new("/etc/hostname");
    let Ok(arquivo) = std::fs::File::open(caminho) else {
        return; // contêiner sem o arquivo
    };
    assert!(conferir_aberto(&Leitor::Usuario { uid: 54_321 }, caminho, &arquivo).is_ok());
    assert!(conferir_aberto(&Leitor::Desconhecido, caminho, &arquivo).is_err());
}

/// Este próprio arquivo de fonte: existe em qualquer máquina que compila o teste.
const ESTE_ARQUIVO: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/src/permissao/testes.rs");

/// Um sistema de mentira, que responde sempre a mesma coisa.
#[derive(Debug)]
struct Sistema(std::io::Result<bool>);

impl Autorizacao for Sistema {
    fn pode_ler(&self, _aberto: &std::fs::File, _pasta: bool) -> std::io::Result<bool> {
        match &self.0 {
            Ok(pode) => Ok(*pode),
            Err(erro) => Err(std::io::Error::new(erro.kind(), "falhou")),
        }
    }
}

fn pelo(resposta: std::io::Result<bool>) -> Leitor {
    Leitor::PeloSistema(Arc::new(Sistema(resposta)))
}

#[test]
fn pelo_sistema_quem_decide_e_o_sistema() {
    let caminho = Path::new(ESTE_ARQUIVO);
    let arquivo = std::fs::File::open(caminho).unwrap();
    let dados = arquivo.metadata().unwrap();
    assert!(conferir_aberto(&pelo(Ok(true)), caminho, &arquivo).is_ok());
    assert!(conferir_aberto(&pelo(Ok(false)), caminho, &arquivo).is_err());
    assert!(conferir(&pelo(Ok(true)), caminho, &dados).is_ok());
    assert!(conferir_entrada(&pelo(Ok(false)), caminho, &dados).is_err());
}

#[test]
fn pelo_sistema_uma_pergunta_que_falha_e_um_nao() {
    let caminho = Path::new(ESTE_ARQUIVO);
    let arquivo = std::fs::File::open(caminho).unwrap();
    let falhou = pelo(Err(std::io::Error::other("sem token")));
    assert!(conferir_aberto(&falhou, caminho, &arquivo).is_err());
}

#[test]
fn pelo_sistema_nao_decide_por_dono_e_modo() {
    assert!(!permite(&pelo(Ok(true)), arquivo(MAXUEL, 0o777), Uso::Ler));
}

#[test]
fn pelo_sistema_uma_pasta_abre_para_ser_perguntada() {
    // No Windows uma pasta não abre como arquivo sem o sinal de *backup semantics*; sem ele, toda
    // pasta copiada era recusada antes de o sistema ser perguntado.
    let pasta = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let dados = std::fs::metadata(&pasta).unwrap();
    assert!(conferir_entrada(&pelo(Ok(true)), &pasta, &dados).is_ok());
    assert!(conferir_entrada(&pelo(Ok(false)), &pasta, &dados).is_err());
}
