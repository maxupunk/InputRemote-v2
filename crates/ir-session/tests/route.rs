//! Cenários da rota dupla: Bluetooth e rede ao mesmo tempo, valendo o que chegar primeiro.
//!
//! O que se prova aqui é o que a [ADR-0012](../../../docs/adr/0012-rota-dupla.md) promete: cada
//! quadro sai pelos dois portadores, a cópia é aplicada uma vez só, e um portador entrar, sair ou
//! morrer calado não derruba a sessão nem solta a tecla que o usuário está segurando.
#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

mod common;

use common::{Pair, Side, is};
use ir_proto::carrier::Carrier;
use ir_proto::ids::RadioAddress;
use ir_proto::input::{HidUsage, PointerDelta};
use ir_session::event::{Command, Injection, LinkDown, Notice};
use ir_session::{Input, Phase, Route};

const TECLA: HidUsage = HidUsage(0x04);

/// Uma sessão de pé com os dois portadores na rota.
fn dual() -> Pair {
    let mut pair = Pair::matched();
    pair.connect(Carrier::Udp);
    pair.connect(Carrier::Rfcomm);
    assert_eq!(pair.server.route(), Some(Route::Dual));
    assert_eq!(pair.client.route(), Some(Route::Dual));
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

fn key(pair: &mut Pair, pressed: bool) {
    pair.feed(
        Side::Server,
        Input::LocalKey {
            usage: TECLA,
            pressed,
        },
    );
}

fn key_injections(pair: &Pair) -> usize {
    pair.count(Side::Client, |command| {
        matches!(command, Command::Inject(Injection::Key { .. }))
    })
}

fn down(pair: &mut Pair, carrier: Carrier) {
    for side in [Side::Server, Side::Client] {
        pair.feed(
            side,
            Input::CarrierDown {
                carrier,
                reason: LinkDown::TransportFailed,
            },
        );
    }
}

#[test]
fn every_frame_leaves_by_both_carriers() {
    let mut pair = dual();
    cross(&mut pair);
    key(&mut pair, true);

    let rfcomm = pair.sent_on(Side::Server, Carrier::Rfcomm);
    assert!(rfcomm > 0);
    assert_eq!(rfcomm, pair.sent_on(Side::Server, Carrier::Udp));
}

#[test]
fn a_key_travels_twice_and_is_typed_once() {
    // A cópia que chega depois é descartada pela detecção de repetição. Aplicá-la seria digitar
    // duas vezes o que o usuário digitou uma (docs/04 §2).
    let mut pair = dual();
    cross(&mut pair);
    key(&mut pair, true);
    key(&mut pair, false);

    assert_eq!(
        key_injections(&pair),
        2,
        "um KeyDown e um KeyUp, nada a mais"
    );
    assert!(pair.client.input_state().is_released());
}

/// Quanto o cursor do cliente anda com um movimento de `dx` no servidor.
fn client_moves_by(pair: &mut Pair, dx: i32) -> i32 {
    let before = pair.client.pointer_xy().0;
    pair.feed(
        Side::Server,
        Input::LocalPointer(PointerDelta { dx, dy: 0 }),
    );
    // O movimento é coalescido e despachado no intervalo do ponteiro.
    pair.advance(10);
    pair.client.pointer_xy().0 - before
}

#[test]
fn a_pointer_sample_moves_the_cursor_once_on_a_dual_route() {
    // O movimento é relativo: sem o filtro de sequência do canal 2, a cópia moveria o cursor o
    // dobro — e toda amostra tem cópia na rota dupla.
    let mut pair = dual();
    cross(&mut pair);
    let _ = client_moves_by(&mut pair, 600);
    assert_eq!(client_moves_by(&mut pair, 40), 40);
}

#[test]
fn a_duplicated_pointer_sample_moves_the_cursor_once_on_a_single_route() {
    // O mesmo defeito existia antes da rota dupla, raro: um datagrama duplicado pela rede.
    let mut pair = Pair::matched();
    pair.connect(Carrier::Udp);
    cross(&mut pair);
    let _ = client_moves_by(&mut pair, 600);
    pair.set_duplicating(true);
    assert_eq!(client_moves_by(&mut pair, 40), 40);
}

#[test]
fn losing_bluetooth_mid_keypress_keeps_the_session_and_the_key() {
    let mut pair = dual();
    cross(&mut pair);
    key(&mut pair, true);
    pair.clear_log();

    down(&mut pair, Carrier::Rfcomm);

    assert!(!pair.any(Side::Server, is::release_all), "nada é solto");
    assert!(!pair.any(Side::Client, is::release_all));
    assert_eq!(pair.server.phase(), Phase::Sending, "a sessão seguiu");
    assert_eq!(pair.server.route(), Some(Route::Single(Carrier::Udp)));
    assert!(!pair.client.input_state().is_released());

    key(&mut pair, false);
    assert!(
        pair.client.input_state().is_released(),
        "o KeyUp chegou pelo portador que sobrou"
    );
}

#[test]
fn a_silent_bluetooth_does_not_cost_the_session() {
    // O rádio sob interferência: para o sistema ele continua de pé, e o que vai por ele não chega.
    // Com a rota dupla a rede cobre, e o prazo de queda conta a rota inteira.
    let mut pair = dual();
    cross(&mut pair);
    pair.set_carrier_delivery(Carrier::Rfcomm, false);
    let before = pair.client.carrier_wins();

    for _ in 0..12 {
        pair.advance(250);
    }
    key(&mut pair, true);
    key(&mut pair, false);

    assert_eq!(
        pair.server.phase(),
        Phase::Sending,
        "3 s sem Bluetooth, e nada caiu"
    );
    assert_eq!(key_injections(&pair), 2);
    let after = pair.client.carrier_wins();
    assert_eq!(after.rfcomm, before.rfcomm, "nada chegou pelo rádio calado");
    assert!(after.udp > before.udp);
    let silence = pair
        .server
        .carrier_silence(Carrier::Rfcomm, pair.now())
        .expect("o rádio ainda está na rota");
    assert!(
        silence.get() >= 3000,
        "o silêncio do rádio é visível: {silence:?}"
    );
}

#[test]
fn with_both_carriers_silent_the_session_falls_and_releases() {
    let mut pair = dual();
    cross(&mut pair);
    key(&mut pair, true);
    pair.set_carrier_delivery(Carrier::Rfcomm, false);
    pair.set_carrier_delivery(Carrier::Udp, false);

    for _ in 0..8 {
        pair.advance(250);
    }

    assert!(pair.client.input_state().is_released(), "cair solta tudo");
    assert!(pair.any(Side::Client, is::release_all));
}

#[test]
fn a_carrier_that_comes_back_rejoins_the_route() {
    let mut pair = dual();
    down(&mut pair, Carrier::Rfcomm);
    assert_eq!(pair.server.route(), Some(Route::Single(Carrier::Udp)));
    pair.clear_log();

    pair.connect(Carrier::Rfcomm);

    assert_eq!(pair.server.route(), Some(Route::Dual));
    assert_eq!(pair.client.route(), Some(Route::Dual));
    assert!(
        !pair
            .notices(Side::Server)
            .iter()
            .any(|n| matches!(n, Notice::CarrierChanged { .. })),
        "voltar à rota não é aperto de mão novo"
    );
}

#[test]
fn losing_the_last_carrier_still_ends_the_session() {
    let mut pair = dual();
    cross(&mut pair);
    key(&mut pair, true);
    down(&mut pair, Carrier::Rfcomm);
    down(&mut pair, Carrier::Udp);

    assert_eq!(pair.server.phase(), Phase::Offline);
    assert!(pair.any(Side::Server, is::release_all));
    assert!(pair.client.input_state().is_released());
}

#[test]
fn a_frame_on_a_carrier_this_side_never_saw_up_is_used_but_does_not_widen() {
    // Só a periferia diz que um portador está de pé, depois de conferir a chave do par. Um quadro
    // chegar por ele não basta para juntá-lo à rota daqui — mas também não é descartado.
    let mut pair = Pair::matched();
    pair.connect(Carrier::Udp);
    pair.feed(Side::Server, Input::CarrierUp(Carrier::Rfcomm));
    cross(&mut pair);
    key(&mut pair, true);

    assert_eq!(pair.server.route(), Some(Route::Dual));
    assert_eq!(pair.client.route(), Some(Route::Single(Carrier::Udp)));
    assert_eq!(key_injections(&pair), 1);
}

#[test]
fn a_handshake_in_progress_absorbs_the_second_carrier() {
    // Recomeçar o aperto de mão porque o Bluetooth apareceu no meio dele só atrasaria a sessão.
    let mut pair = Pair::matched();
    pair.set_delivery(false);
    pair.feed(Side::Server, Input::CarrierUp(Carrier::Udp));
    pair.feed(Side::Server, Input::CarrierUp(Carrier::Rfcomm));

    let handshakes = pair
        .notices(Side::Server)
        .iter()
        .filter(|n| matches!(n, Notice::CarrierChanged { .. }))
        .count();
    assert_eq!(handshakes, 1);

    pair.set_delivery(true);
    pair.feed(Side::Client, Input::CarrierUp(Carrier::Udp));
    pair.feed(Side::Client, Input::CarrierUp(Carrier::Rfcomm));
    pair.advance(50);

    assert!(pair.server.phase().is_established());
    assert_eq!(pair.server.route(), Some(Route::Dual));
    assert_eq!(pair.client.route(), Some(Route::Dual));
}

#[test]
fn a_pinned_session_never_goes_dual_and_unpinning_widens_it() {
    let mut pair = Pair::matched();
    pair.pin(Side::Server, Some(Carrier::Udp));
    pair.pin(Side::Client, Some(Carrier::Udp));
    pair.connect(Carrier::Udp);
    pair.connect(Carrier::Rfcomm);
    assert_eq!(pair.server.route(), Some(Route::Single(Carrier::Udp)));

    pair.pin(Side::Server, None);
    assert_eq!(pair.server.route(), Some(Route::Dual));
    assert_eq!(
        pair.server.phase(),
        Phase::Ready,
        "soltar não refaz a sessão"
    );
}

#[test]
fn pinning_a_carrier_of_the_dual_route_narrows_it_in_place() {
    let mut pair = dual();
    cross(&mut pair);
    key(&mut pair, true);
    pair.clear_log();

    pair.pin(Side::Server, Some(Carrier::Rfcomm));

    assert_eq!(pair.server.route(), Some(Route::Single(Carrier::Rfcomm)));
    assert_eq!(pair.server.phase(), Phase::Sending);
    assert!(!pair.any(Side::Server, is::release_all));
}

#[test]
fn reach_tells_the_peer_where_the_radio_is() {
    let radio = RadioAddress([0x74, 0x13, 0xEA, 0xA6, 0x5A, 0x99]);
    let mut pair = Pair::matched();
    pair.feed(Side::Server, Input::LocalRadio(radio));
    pair.connect(Carrier::Udp);

    assert!(
        pair.notices(Side::Client)
            .contains(&Notice::PeerRadio(radio)),
        "quem só conhece o par pela rede fica sabendo para onde discar o Bluetooth"
    );
    assert!(
        !pair
            .notices(Side::Server)
            .iter()
            .any(|n| matches!(n, Notice::PeerRadio(_))),
        "o cliente não disse rádio nenhum"
    );
}

#[test]
fn a_radio_learned_after_the_session_is_told_right_away() {
    let radio = RadioAddress([0xAC, 0x50, 0xDE, 0x47, 0xEB, 0x28]);
    let mut pair = Pair::matched();
    pair.connect(Carrier::Udp);
    pair.clear_log();

    pair.feed(Side::Client, Input::LocalRadio(radio));

    assert!(
        pair.notices(Side::Server)
            .contains(&Notice::PeerRadio(radio))
    );
}
