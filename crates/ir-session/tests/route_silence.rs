//! O portador calado da rota dupla: sai da rota sem derrubar o enlace, e volta pela sonda.
//!
//! A bancada do log 80: o Windows com a rede fixada aceitava o Bluetooth que o Linux discava e
//! nunca mandava por ele, e o Linux mandava cada quadro pelos dois sem perceber.
#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

mod common;

use common::{Pair, Side, is};
use ir_proto::carrier::Carrier;
use ir_proto::input::PointerDelta;
use ir_session::{Input, Phase, Route};

/// Uma sessão de pé com os dois portadores na rota.
fn dual() -> Pair {
    let mut pair = Pair::matched();
    pair.connect(Carrier::Udp);
    pair.connect(Carrier::Rfcomm);
    assert_eq!(pair.server.route(), Some(Route::Dual));
    pair.clear_log();
    pair
}

/// Leva o controle ao cliente.
fn cross(pair: &mut Pair) {
    pair.feed(
        Side::Server,
        Input::LocalPointer(PointerDelta { dx: 5000, dy: 0 }),
    );
    assert_eq!(pair.server.phase(), Phase::Sending);
    pair.clear_log();
}
#[test]
fn a_carrier_the_peer_never_sends_on_leaves_the_route_without_dropping_the_link() {
    // A bancada (log 80): o Windows com a rede fixada aceitava o Bluetooth que o Linux discava, e
    // nunca mandava por ele. O Linux mandava cada quadro pelos dois, sem nunca perceber.
    let mut pair = dual();
    cross(&mut pair);
    assert_eq!(pair.server.mute_carrier(pair.now()), None, "os dois falam");
    pair.set_carrier_delivery(Carrier::Udp, false);

    pair.advance(250);
    assert_eq!(
        pair.server.route(),
        Some(Route::Dual),
        "um quarto de segundo é um pico, não um portador abandonado"
    );
    for _ in 0..12 {
        pair.advance(250);
    }
    assert_eq!(pair.server.route(), Some(Route::Single(Carrier::Rfcomm)));
    assert_eq!(pair.client.route(), Some(Route::Single(Carrier::Rfcomm)));
    assert_eq!(pair.server.phase(), Phase::Sending, "a sessão seguiu");
    assert!(!pair.any(Side::Server, is::release_all), "nada é solto");
    pair.clear_log();
    pair.advance(250);
    assert!(
        pair.sent_on(Side::Server, Carrier::Udp) > 0,
        "o enlace continua de pé, e é sondado a cada batida"
    );
}

#[test]
fn a_frame_on_the_dropped_carrier_brings_it_back() {
    let mut pair = dual();
    pair.set_carrier_delivery(Carrier::Udp, false);
    for _ in 0..13 {
        pair.advance(250);
    }
    assert_eq!(pair.server.route(), Some(Route::Single(Carrier::Rfcomm)));
    pair.set_carrier_delivery(Carrier::Udp, true);

    // Os dois tiraram a rede da rota, e nenhum manda quadro nenhum por ela: é a sonda de cada
    // batida que a devolve assim que ela volta.
    pair.advance(250);
    assert_eq!(pair.server.route(), Some(Route::Dual));
    assert_eq!(pair.client.route(), Some(Route::Dual));
}

#[test]
fn against_a_peer_pinned_to_the_network_the_radio_leaves_the_route_and_is_never_probed_back() {
    // A bancada (log 80): o Windows fixado na rede, o Linux no automático.
    let mut pair = Pair::matched();
    pair.pin(Side::Server, Some(Carrier::Udp));
    pair.connect(Carrier::Udp);
    pair.connect(Carrier::Rfcomm);
    assert_eq!(pair.server.route(), Some(Route::Single(Carrier::Udp)));
    assert_eq!(pair.client.route(), Some(Route::Dual));

    for _ in 0..13 {
        pair.advance(250);
    }
    assert_eq!(
        pair.client.route(),
        Some(Route::Single(Carrier::Udp)),
        "o par nunca manda pelo rádio: mandar cada quadro por ele só gastava o rádio"
    );
    pair.clear_log();
    for _ in 0..4 {
        pair.advance(250);
    }
    assert_eq!(
        pair.sent_on(Side::Server, Carrier::Rfcomm),
        0,
        "quem fixou não sonda"
    );
    assert_eq!(
        pair.client.route(),
        Some(Route::Single(Carrier::Udp)),
        "a sonda do lado automático não convence quem fixou, e não há vaivém"
    );
}

#[test]
fn with_both_carriers_silent_none_is_named_mute_and_the_session_falls() {
    let mut pair = dual();
    pair.set_carrier_delivery(Carrier::Rfcomm, false);
    pair.set_carrier_delivery(Carrier::Udp, false);
    pair.advance_silently(800);
    assert_eq!(
        pair.server.mute_carrier(pair.now()),
        None,
        "sem nenhum vivo, quem cuida é o prazo de queda"
    );
}
