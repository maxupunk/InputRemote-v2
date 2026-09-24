//! Cenários de enlace: handshake, queda, reconexão e troca de portador.
//!
//! Linhas correspondentes de `docs/10-testes-e-validacao.md` §2: reconexão, troca de
//! portador, par que some, e a sessão parada que não pode cair sozinha.
#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

mod common;

use common::{Pair, Side, is};
use ir_proto::carrier::Carrier;
use ir_proto::input::{HidUsage, PointerDelta};
use ir_session::event::{CarrierChoice, LinkDown, Notice};
use ir_session::{Input, Phase, Route};

/// Leva o ponteiro do servidor até a borda direita e atravessa.
fn cross_to_client(pair: &mut Pair) {
    pair.feed(
        Side::Server,
        Input::LocalPointer(PointerDelta { dx: 5000, dy: 0 }),
    );
}

fn connected() -> Pair {
    let mut pair = Pair::matched();
    pair.connect(Carrier::Udp);
    pair.clear_log();
    pair
}

#[test]
fn the_handshake_establishes_a_session_on_both_sides() {
    let mut pair = Pair::matched();
    pair.connect(Carrier::Udp);

    assert_eq!(pair.server.phase(), Phase::Ready);
    assert_eq!(pair.client.phase(), Phase::Ready);
    assert_eq!(
        pair.server.peer().map(|p| p.name.as_str().to_owned()),
        Some("cliente".into())
    );
    assert_eq!(
        pair.client.peer().map(|p| p.name.as_str().to_owned()),
        Some("servidor".into())
    );

    let announced = pair
        .notices(Side::Server)
        .iter()
        .any(|n| matches!(n, Notice::Connected { .. }));
    assert!(announced, "a interface precisa saber que conectou");
}

#[test]
fn the_link_falls_by_timeout_within_the_promised_second() {
    let mut pair = connected();
    cross_to_client(&mut pair);
    pair.set_delivery(false); // o par sumiu: nada mais chega
    pair.clear_log();

    pair.advance(1100);

    assert_eq!(
        pair.client.phase(),
        Phase::Offline,
        "o cliente precisa desistir"
    );
    assert!(
        pair.any(Side::Client, is::release_all),
        "e soltar tudo ao desistir"
    );
    assert!(pair.client.input_state().is_released());
}

#[test]
fn a_dropped_link_that_may_return_asks_for_a_reconnect_timer() {
    let mut pair = connected();
    pair.clear_log();

    pair.feed(
        Side::Server,
        Input::CarrierDown {
            carrier: Carrier::Udp,
            reason: LinkDown::TransportFailed,
        },
    );

    let retrying = pair.notices(Side::Server).iter().any(|n| {
        matches!(
            n,
            Notice::Disconnected {
                will_retry: true,
                ..
            }
        )
    });
    assert!(
        retrying,
        "queda transitória precisa anunciar que vai tentar de novo"
    );
}

#[test]
fn a_user_stop_does_not_try_to_reconnect() {
    let mut pair = connected();
    pair.clear_log();

    pair.feed(
        Side::Server,
        Input::CarrierDown {
            carrier: Carrier::Udp,
            reason: LinkDown::UserStopped,
        },
    );

    let retrying = pair.notices(Side::Server).iter().any(|n| {
        matches!(
            n,
            Notice::Disconnected {
                will_retry: true,
                ..
            }
        )
    });
    assert!(
        !retrying,
        "reconectar depois de o usuário mandar parar seria desobedecer"
    );
}

#[test]
fn the_session_comes_back_after_a_drop_without_any_new_pairing() {
    let mut pair = connected();
    pair.feed(
        Side::Server,
        Input::CarrierDown {
            carrier: Carrier::Udp,
            reason: LinkDown::TransportFailed,
        },
    );
    pair.feed(
        Side::Client,
        Input::CarrierDown {
            carrier: Carrier::Udp,
            reason: LinkDown::TransportFailed,
        },
    );
    assert_eq!(pair.server.phase(), Phase::Offline);
    pair.clear_log();

    pair.connect(Carrier::Udp);

    assert_eq!(pair.server.phase(), Phase::Ready, "o enlace volta sozinho");
    assert_eq!(pair.client.phase(), Phase::Ready);
}

#[test]
fn bluetooth_joins_the_network_route_without_a_new_handshake() {
    // Antes da rota dupla, o Bluetooth aparecendo derrubava a sessão da rede e refazia o aperto de
    // mão por ele. Agora ele entra na rota da sessão que já está de pé (ADR-0012).
    let mut pair = connected();
    pair.connect(Carrier::Rfcomm);

    for side in [Side::Server, Side::Client] {
        let session = if side == Side::Server {
            &pair.server
        } else {
            &pair.client
        };
        assert_eq!(session.route(), Some(Route::Dual), "{side:?}");
        assert_eq!(session.phase(), Phase::Ready, "{side:?}: nada recomeçou");
    }
    assert!(
        !pair
            .notices(Side::Server)
            .iter()
            .any(|n| matches!(n, Notice::CarrierChanged { .. })),
        "não houve aperto de mão novo"
    );
    let explained = pair.notices(Side::Server).iter().any(|n| {
        matches!(
            n,
            Notice::RouteChanged {
                route: Route::Dual,
                why: CarrierChoice::Redundant,
            }
        )
    });
    assert!(explained, "a mudança precisa ser visível, com o motivo");
}

#[test]
fn bluetooth_joining_while_remote_keeps_the_keys_held() {
    // Juntar um portador não é trocar de meio: nada em trânsito se perde, então não há o que
    // soltar. Soltar aqui cortaria a tecla que o usuário está segurando.
    let mut pair = connected();
    cross_to_client(&mut pair);
    pair.feed(
        Side::Server,
        Input::LocalKey {
            usage: HidUsage(0x04),
            pressed: true,
        },
    );
    pair.clear_log();

    pair.connect(Carrier::Rfcomm);

    assert!(!pair.any(Side::Server, is::release_all));
    assert!(!pair.any(Side::Client, is::release_all));
    assert_eq!(pair.server.phase(), Phase::Sending);
    assert!(
        !pair.client.input_state().is_released(),
        "a tecla continua segura"
    );
}

#[test]
fn switching_to_a_pinned_carrier_while_remote_releases_everything_first() {
    // Fixado, não há rota dupla: aparecer o portador fixado ainda é troca de meio, com aperto de
    // mão novo — e aí o risco de tecla presa volta, e a regra de soltar antes também.
    let mut pair = connected();
    pair.pin(Side::Server, Some(Carrier::Rfcomm));
    cross_to_client(&mut pair);
    pair.feed(
        Side::Server,
        Input::LocalKey {
            usage: HidUsage(0x04),
            pressed: true,
        },
    );
    pair.clear_log();

    pair.feed(Side::Server, Input::CarrierUp(Carrier::Rfcomm));

    assert!(
        pair.any(Side::Server, is::release_all),
        "trocar de meio com tecla presa é o risco"
    );
    assert!(pair.server.input_state().is_released());
    assert!(pair.any(Side::Server, is::unsuppress));
    assert_eq!(pair.server.carrier(), Some(Carrier::Rfcomm));
}

#[test]
fn the_heartbeat_keeps_a_quiet_session_alive_well_past_the_timeout() {
    let mut pair = connected();
    // Muito além do prazo de queda de 1 s, mas com os dois lados respondendo.
    for _ in 0..40 {
        pair.advance(250);
    }
    assert_eq!(
        pair.server.phase(),
        Phase::Ready,
        "uma sessão parada não pode cair sozinha"
    );
    assert_eq!(pair.client.phase(), Phase::Ready);
}

#[test]
fn latency_is_measured_and_reported() {
    let mut pair = connected();
    pair.clear_log();
    // Precisa passar do intervalo de heartbeat para o `Ping` sair.
    pair.advance(250);

    let measured = pair
        .notices(Side::Server)
        .iter()
        .any(|n| matches!(n, Notice::LatencySample(_)));
    assert!(
        measured,
        "sem amostra de latência não há como observar o estado"
    );
}
