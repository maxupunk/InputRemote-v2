//! "Copiar e colar" é uma escolha dos dois computadores: quem muda num lado muda no outro.
//!
//! O defeito relatado: desligado no Linux, o Ctrl+C do Windows continuava saindo, era recusado lá,
//! e o próprio Linux mostrava "copiar e colar está desligado no outro computador".
#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

mod common;

use common::{Pair, Side, layout};
use ir_proto::carrier::Carrier;
use ir_proto::screens::Edge;
use ir_session::event::Notice;
use ir_session::{CopyPaste, Input, SessionConfig};

const fn escolha(enabled: bool, chosen_at: u64) -> CopyPaste {
    CopyPaste { enabled, chosen_at }
}

fn adotadas(pair: &Pair, side: Side) -> Vec<CopyPaste> {
    pair.notices(side)
        .iter()
        .filter_map(|notice| match notice {
            Notice::CopyPasteAdopted(choice) => Some(*choice),
            _ => None,
        })
        .collect()
}

fn com(server: CopyPaste, client: CopyPaste) -> Pair {
    let mut a = SessionConfig::new(Edge::Right);
    a.copy_paste = server;
    let mut b = SessionConfig::new(Edge::Left);
    b.copy_paste = client;
    Pair::with_configs(a, b, layout(1920, 1080), layout(1920, 1080))
}

#[test]
fn desligar_num_lado_desliga_o_outro_na_sessao_de_pe() {
    let mut pair = com(CopyPaste::DEFAULT, CopyPaste::DEFAULT);
    pair.connect(Carrier::Udp);
    pair.clear_log();

    pair.feed(Side::Client, Input::SetCopyPaste(escolha(false, 500)));
    pair.advance(50);

    assert_eq!(pair.server.copy_paste(), escolha(false, 500));
    assert_eq!(adotadas(&pair, Side::Server), vec![escolha(false, 500)]);
    assert!(
        adotadas(&pair, Side::Client).is_empty(),
        "quem escolheu não adota a própria escolha"
    );

    // E religar, de qualquer lado, religa os dois.
    pair.feed(Side::Server, Input::SetCopyPaste(escolha(true, 900)));
    pair.advance(50);
    assert_eq!(pair.client.copy_paste(), escolha(true, 900));
}

#[test]
fn ao_conectar_vale_a_escolha_mais_recente() {
    let mut pair = com(escolha(true, 800), escolha(false, 300));
    pair.connect(Carrier::Udp);
    pair.advance(50);

    assert_eq!(pair.client.copy_paste(), escolha(true, 800));
    assert_eq!(pair.server.copy_paste(), escolha(true, 800));
    assert!(adotadas(&pair, Side::Server).is_empty());
}

/// Quem atualiza com a opção já desligada num lado: nenhum dos dois tem horário, e o desligado é
/// o que alguém escolheu — o caso exato da bancada.
#[test]
fn sem_horario_dos_dois_lados_vale_desligado() {
    let mut pair = com(escolha(true, 0), escolha(false, 0));
    pair.connect(Carrier::Udp);
    pair.advance(50);

    assert!(!pair.server.copy_paste().enabled);
    assert!(!pair.client.copy_paste().enabled);
    assert_eq!(adotadas(&pair, Side::Server), vec![escolha(false, 0)]);
}

#[test]
fn escolha_igual_nos_dois_nao_gera_aviso() {
    let mut pair = com(escolha(false, 100), escolha(false, 700));
    pair.connect(Carrier::Udp);
    pair.advance(50);

    assert!(adotadas(&pair, Side::Server).is_empty());
    assert!(adotadas(&pair, Side::Client).is_empty());
}
