#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::Path;

use crate::config::Config;

use ir_session::{Command, Notice};

use super::*;
use crate::actor::bancada::{Bancada, CapturaDeMentira, Diretorio, diretorio};

fn daemon(papel: Role) -> (Daemon, Diretorio) {
    let bancada = Bancada::nova(papel);
    (bancada.daemon, bancada.dir)
}

/// Um serviço que consegue capturar, para poder virar o que tem o teclado em qualquer máquina.
fn capaz_de_capturar(papel: Role) -> (Daemon, Diretorio) {
    let (mut daemon, dir) = daemon(papel);
    daemon.capturer = Some(Box::new(CapturaDeMentira));
    (daemon, dir)
}

fn gravado(dir: &Path) -> String {
    std::fs::read_to_string(dir.join("config.toml")).unwrap_or_default()
}

#[test]
fn o_papel_que_o_par_escolheu_depois_e_adotado_gravado_e_contado() {
    let (mut daemon, dir) = capaz_de_capturar(Role::Client);
    let mut avisos = daemon.avisos.subscribe();
    daemon.out.push(Command::Notify(Notice::AdoptRole {
        role: Role::Server,
        chosen_at: 500,
    }));
    daemon.apply_commands();
    daemon.gravador.esperar();

    assert_eq!(daemon.session.role(), Role::Server);
    let arquivo = gravado(&dir);
    assert!(arquivo.contains(r#"role = "server""#), "{arquivo}");
    // O horário do par, e não o de agora: senão esta ponta cederia de volta na próxima vez.
    assert!(arquivo.contains("papel_escolhido_em = 500"), "{arquivo}");
    let contou = std::iter::from_fn(|| avisos.try_recv().ok())
        .any(|aviso| aviso == Aviso::PapelAjustado(ir_ipc::Papel::Servidor));
    assert!(contou, "a tela conta por que o papel mudou");
}

#[test]
fn adotar_o_papel_que_ja_tem_nao_refaz_nada() {
    let (mut daemon, dir) = daemon(Role::Server);
    daemon.adotar_papel(Role::Server, 500);
    daemon.gravador.esperar();
    assert!(!gravado(&dir).contains("papel_escolhido_em"));
}

#[test]
fn trocar_o_papel_pela_tela_grava_quando() {
    let (mut daemon, _dir) = capaz_de_capturar(Role::Client);
    assert_eq!(daemon.trocar_papel(Role::Server), Resposta::Feito);
    assert!(daemon.config.papel_escolhido_em.is_some_and(|t| t > 0));
}

#[test]
fn servidor_so_onde_ha_captura() {
    assert!(!papel_sustentado(Role::Server, false));
    assert!(papel_sustentado(Role::Server, true));
    assert!(papel_sustentado(Role::Client, false));
    assert!(papel_sustentado(Role::Client, true));
}

#[test]
fn a_borda_nova_ja_vale_na_sessao_em_uso() {
    // O defeito era a janela mostrar a borda nova e a travessia usar a velha.
    let (mut daemon, dir) = daemon(Role::Server);
    assert_eq!(daemon.trocar_borda(Edge::Left), Resposta::Feito);
    assert_eq!(daemon.session.peer_edge(), Edge::Left);
    assert!(
        gravado(&dir).contains(r#"peer_edge = "left""#),
        "{}",
        gravado(&dir)
    );
}

#[test]
fn trocar_a_borda_nao_refaz_a_sessao() {
    // Refazer mandava ao par um adeus de "encerrada pelo usuário", que ele não reconecta. Aqui
    // não há enlace: uma sessão refeita voltaria desligada, e a em uso continua no aperto de mão.
    let (mut daemon, _dir) = daemon(Role::Server);
    daemon.drive(Input::CarrierUp(Carrier::Udp));
    assert_eq!(daemon.session.phase(), Phase::Handshaking);

    assert_eq!(daemon.trocar_borda(Edge::Left), Resposta::Feito);

    assert_eq!(
        daemon.session.phase(),
        Phase::Handshaking,
        "a sessão é a mesma"
    );
    assert_eq!(daemon.session.peer_edge(), Edge::Left);
}

#[test]
fn no_cliente_a_borda_e_do_servidor_e_nada_e_gravado() {
    let (mut daemon, dir) = daemon(Role::Client);
    assert_eq!(
        daemon.trocar_borda(Edge::Left),
        Resposta::Falha(Falha::BordaDoServidor)
    );
    assert_eq!(daemon.session.peer_edge(), Edge::Right, "a sessão não muda");
    assert!(
        !dir.join("config.toml").exists(),
        "uma recusa não pode gravar nada"
    );
}

#[test]
fn o_cliente_grava_a_borda_que_o_servidor_anunciou() {
    // Sem gravar, o cliente subiria com a borda velha e atravessaria errado até reconectar.
    let (mut daemon, dir) = daemon(Role::Client);
    daemon
        .out
        .push(Command::Notify(Notice::EdgeChanged { edge: Edge::Left }));
    daemon.apply_commands();
    daemon.gravador.esperar();

    assert!(
        gravado(&dir).contains(r#"peer_edge = "left""#),
        "{}",
        gravado(&dir)
    );
    assert_eq!(daemon.estado().borda_do_par, ir_ipc::Borda::Esquerda);
}

#[test]
fn o_papel_novo_ja_vale_na_sessao_em_uso() {
    // Voltar a cliente é sempre possível — é o caminho de quem ficou servidor por engano.
    let (mut daemon, dir) = daemon(Role::Server);
    assert_eq!(daemon.trocar_papel(Role::Client), Resposta::Feito);
    assert_eq!(daemon.session.role(), Role::Client);
    assert!(
        gravado(&dir).contains(r#"role = "client""#),
        "{}",
        gravado(&dir)
    );
}

#[test]
fn servidor_sem_captura_e_recusado_sem_gravar_nada() {
    if ir_input::capture_supported() {
        return; // aqui o servidor é legítimo
    }
    let (mut daemon, dir) = daemon(Role::Client);
    assert_eq!(
        daemon.trocar_papel(Role::Server),
        Resposta::Falha(Falha::PapelIndisponivel)
    );
    assert_eq!(daemon.session.role(), Role::Client, "a sessão não muda");
    assert!(
        !dir.join("config.toml").exists(),
        "uma recusa não pode gravar nada"
    );
}

#[test]
fn na_subida_um_servidor_sem_captura_vira_cliente_e_o_arquivo_e_corrigido() {
    let dir = diretorio();
    let mut config = Config {
        role: texto_do_papel(Role::Server).to_owned(),
        ..Config::default()
    };
    let papel = crate::config::papel_na_subida(&mut config, &dir, ir_input::capture_supported())
        .expect("papel reconhecido");
    if ir_input::capture_supported() {
        assert_eq!(papel, Role::Server, "onde há captura, o gravado vale");
    } else {
        assert_eq!(papel, Role::Client);
        assert!(
            gravado(&dir).contains(r#"role = "client""#),
            "{}",
            gravado(&dir)
        );
    }
}
