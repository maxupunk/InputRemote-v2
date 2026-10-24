//! O painel no Android: a amostra de uso que um computador pede ao outro, e a latência por rota.
//!
//! O que se prova: o pedido e a amostra só andam com sessão de pé; a amostra chega dentro dos
//! tetos; e na rota dupla cada portador tem a sua volta medida, embora só um `Pong` seja aplicado.
#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

mod common;

use common::{Pair, Side};
use ir_proto::carrier::Carrier;
use ir_proto::message::UsageSample;
use ir_session::event::Notice;
use ir_session::{Input, Route};

fn pedidos(pair: &Pair, side: Side) -> Vec<bool> {
    pair.notices(side)
        .iter()
        .filter_map(|notice| match notice {
            Notice::PeerWatchesUsage(watch) => Some(*watch),
            _ => None,
        })
        .collect()
}

fn amostras(pair: &Pair, side: Side) -> Vec<UsageSample> {
    pair.notices(side)
        .iter()
        .filter_map(|notice| match notice {
            Notice::PeerUsage(sample) => Some(sample.clone()),
            _ => None,
        })
        .collect()
}

fn rotas_medidas(pair: &Pair, side: Side) -> Vec<Carrier> {
    let mut carriers: Vec<Carrier> = pair
        .notices(side)
        .iter()
        .filter_map(|notice| match notice {
            Notice::RouteLatency { carrier, .. } => Some(*carrier),
            _ => None,
        })
        .collect();
    carriers.sort_unstable();
    carriers.dedup();
    carriers
}

/// Um segundo em batidas de 250 ms — um avanço só de 1 s passaria do prazo de queda.
fn batidas(pair: &mut Pair) {
    for _ in 0..4 {
        pair.advance(250);
    }
}

#[test]
fn o_pedido_chega_ao_par_e_a_amostra_volta() {
    let mut pair = Pair::matched();
    pair.connect(Carrier::Udp);
    pair.clear_log();
    assert!(pair.server.can_watch_peer_usage());

    pair.feed(Side::Server, Input::WatchPeerUsage(true));
    pair.advance(50);
    assert_eq!(pedidos(&pair, Side::Client), vec![true]);

    let sample = UsageSample {
        cpu_permille: Some(120),
        ..UsageSample::default()
    };
    pair.feed(Side::Client, Input::ShareUsage(sample.clone()));
    pair.advance(50);
    assert_eq!(amostras(&pair, Side::Server), vec![sample]);
    assert!(
        amostras(&pair, Side::Client).is_empty(),
        "a amostra não volta"
    );
}

#[test]
fn a_amostra_impossivel_chega_dentro_dos_tetos() {
    let mut pair = Pair::matched();
    pair.connect(Carrier::Udp);
    pair.clear_log();

    let impossivel = UsageSample {
        cpu_permille: Some(60_000),
        ..UsageSample::default()
    };
    pair.feed(Side::Client, Input::ShareUsage(impossivel));
    pair.advance(50);
    let chegou = amostras(&pair, Side::Server);
    assert_eq!(chegou.first().and_then(|s| s.cpu_permille), Some(1000));
}

#[test]
fn sem_sessao_nao_ha_a_quem_pedir() {
    let mut pair = Pair::matched();
    assert!(!pair.server.can_watch_peer_usage());
    pair.feed(Side::Server, Input::WatchPeerUsage(true));
    pair.feed(Side::Server, Input::ShareUsage(UsageSample::default()));
    pair.advance(50);
    assert!(pedidos(&pair, Side::Client).is_empty());
    assert!(amostras(&pair, Side::Client).is_empty());
}

#[test]
fn na_rota_dupla_cada_portador_tem_a_sua_volta() {
    let mut pair = Pair::matched();
    pair.connect(Carrier::Udp);
    pair.connect(Carrier::Rfcomm);
    assert_eq!(pair.server.route(), Some(Route::Dual));
    pair.clear_log();

    // Algumas batidas, abaixo do prazo de queda: o `Ping` sai pelos dois, e o `Pong` volta pelos
    // dois.
    batidas(&mut pair);

    assert_eq!(
        rotas_medidas(&pair, Side::Server),
        vec![Carrier::Rfcomm, Carrier::Udp]
    );
}

#[test]
fn com_um_portador_so_ele_e_medido() {
    let mut pair = Pair::matched();
    pair.connect(Carrier::Udp);
    pair.clear_log();
    batidas(&mut pair);
    assert_eq!(rotas_medidas(&pair, Side::Server), vec![Carrier::Udp]);
}
