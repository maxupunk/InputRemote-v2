#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::Path;

use ir_session::{Command, Notice};

use super::*;
use crate::actor::bancada::{Bancada, CapturaDeMentira, Diretorio, diretorio};
use crate::config::Config;

fn daemon() -> (Daemon, Diretorio) {
    let bancada = Bancada::nova();
    (bancada.daemon, bancada.dir)
}

/// Um serviço que consegue capturar, para poder controlar o outro em qualquer máquina.
fn capaz_de_capturar() -> (Daemon, Diretorio) {
    let (mut daemon, dir) = daemon();
    daemon.capturer = Some(Box::new(CapturaDeMentira));
    (daemon, dir)
}

fn gravado(dir: &Path) -> String {
    std::fs::read_to_string(dir.join("config.toml")).unwrap_or_default()
}

#[test]
fn por_padrao_os_dois_controlam_um_ao_outro() {
    let (daemon, _dir) = daemon();
    assert_eq!(daemon.session.policy(), Policy::Both);
    assert_eq!(daemon.estado().politica, ir_ipc::Politica::Ambos);
}

#[test]
fn a_politica_nova_ja_vale_e_fica_gravada() {
    let (mut daemon, dir) = daemon();
    assert_eq!(
        daemon.definir_politica(Policy::OnlyControlled),
        Resposta::Feito
    );
    daemon.gravador.esperar();
    assert_eq!(daemon.session.policy(), Policy::OnlyControlled);
    assert!(
        gravado(&dir).contains(r#"politica = "so-o-outro""#),
        "{}",
        gravado(&dir)
    );
}

#[test]
fn a_mesma_politica_nao_refaz_nada() {
    let (mut daemon, dir) = daemon();
    assert_eq!(daemon.definir_politica(Policy::Both), Resposta::Feito);
    assert!(!dir.join("config.toml").exists(), "nada a gravar");
}

#[test]
fn so_este_controla_exige_ler_o_teclado_daqui() {
    // Aceitar e só depois descobrir que a captura não sobe deixaria a máquina sem ter o que mandar
    // (log 47). Com captura, a política vale.
    let (mut com, _dir) = capaz_de_capturar();
    if ir_input::capture_supported() {
        assert_eq!(com.definir_politica(Policy::OnlyControls), Resposta::Feito);
        assert_eq!(com.session.policy(), Policy::OnlyControls);
    } else {
        assert_eq!(
            com.definir_politica(Policy::OnlyControls),
            Resposta::Falha(Falha::PoliticaIndisponivel)
        );
    }
}

#[test]
fn a_sessao_refeita_guarda_o_ponteiro_onde_estava() {
    // Onde o serviço conduz o cursor, uma sessão nova no canto faria o cursor saltar.
    let (mut daemon, _dir) = daemon();
    daemon.definir_telas(ir_proto::screens::ScreenLayout::single(1920, 1080).unwrap());
    daemon.session.sync_pointer(700, 300);
    assert_eq!(
        daemon.definir_politica(Policy::OnlyControlled),
        Resposta::Feito
    );
    assert_eq!(daemon.session.pointer_xy(), (700, 300));
}

#[test]
fn a_borda_nova_ja_vale_na_sessao_em_uso_e_fica_gravada_com_o_horario() {
    // O defeito era a janela mostrar a borda nova e a travessia usar a velha.
    let (mut daemon, dir) = daemon();
    assert_eq!(daemon.trocar_borda(Edge::Left), Resposta::Feito);
    daemon.gravador.esperar();
    assert_eq!(daemon.session.peer_edge(), Edge::Left);
    let arquivo = gravado(&dir);
    assert!(arquivo.contains(r#"peer_edge = "left""#), "{arquivo}");
    assert!(arquivo.contains("borda_escolhida_em"), "{arquivo}");
}

#[test]
fn trocar_a_borda_nao_refaz_a_sessao() {
    // Refazer mandava ao par um adeus de "encerrada pelo usuário", que ele não reconecta. Aqui
    // não há enlace: uma sessão refeita voltaria desligada, e a em uso continua no aperto de mão.
    let (mut daemon, _dir) = daemon();
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
fn a_borda_que_o_par_escolheu_e_gravada_com_o_horario_dele_e_contada() {
    // Sem gravar, esta máquina subiria com a borda velha; com o horário de agora, ela venceria a
    // próxima comparação e o par cederia de volta.
    let (mut daemon, dir) = daemon();
    let mut avisos = daemon.avisos.subscribe();
    daemon.out.push(Command::Notify(Notice::EdgeAdopted {
        edge: Edge::Left,
        chosen_at: 500,
    }));
    daemon.apply_commands();
    daemon.gravador.esperar();

    let arquivo = gravado(&dir);
    assert!(arquivo.contains(r#"peer_edge = "left""#), "{arquivo}");
    assert!(arquivo.contains("borda_escolhida_em = 500"), "{arquivo}");
    assert_eq!(daemon.estado().borda_do_par, ir_ipc::Borda::Esquerda);
    let contou = std::iter::from_fn(|| avisos.try_recv().ok())
        .any(|aviso| aviso == Aviso::BordaAjustada(ir_ipc::Borda::Esquerda));
    assert!(contou, "a tela conta por que a posição mudou");
}

#[test]
fn na_subida_uma_politica_que_a_plataforma_nao_sustenta_e_corrigida() {
    let dir = diretorio();
    let mut config = Config {
        politica: "so-este".to_owned(),
        ..Config::default()
    };
    let politica =
        crate::config::politica_na_subida(&mut config, &dir, ir_input::capture_supported())
            .expect("política reconhecida");
    if ir_input::capture_supported() {
        assert_eq!(
            politica,
            Policy::OnlyControls,
            "onde há captura, a gravada vale"
        );
    } else {
        assert_eq!(politica, Policy::Both);
        assert!(
            gravado(&dir).contains(r#"politica = "ambos""#),
            "{}",
            gravado(&dir)
        );
    }
}
