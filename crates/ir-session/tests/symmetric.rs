//! O controle simétrico: qualquer um dos dois controla o outro, e quem mexe por último, manda
//! (ADR-0014).
//!
//! Na bancada, `server` é o computador A (identificador 1, o par à direita) e `client` o B
//! (identificador 2, o par à esquerda). São nomes de posição: os dois fazem as duas coisas.
#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

mod common;

use common::{Pair, Side, is, layout};
use ir_proto::carrier::Carrier;
use ir_proto::input::{Button, HidUsage, PointerDelta};
use ir_proto::screens::Edge;
use ir_session::event::Notice;
use ir_session::{Command, Input, Phase, Policy, RECLAIM_DISTANCE, SessionConfig};

const A_KEY: HidUsage = HidUsage(0x04);

fn connected() -> Pair {
    let mut pair = Pair::matched();
    pair.connect(Carrier::Udp);
    pair.clear_log();
    pair
}

fn with_policies(a: Policy, b: Policy) -> Pair {
    let mut config_a = SessionConfig::new(Edge::Right);
    config_a.policy = a;
    let mut config_b = SessionConfig::new(Edge::Left);
    config_b.policy = b;
    let mut pair = Pair::with_configs(config_a, config_b, layout(1920, 1080), layout(1920, 1080));
    pair.connect(Carrier::Udp);
    pair.clear_log();
    pair
}

fn push(pair: &mut Pair, side: Side, dx: i32) {
    pair.feed(side, Input::LocalPointer(PointerDelta { dx, dy: 0 }));
}

/// A leva o controle a B pela direita, e o tempo de graça passa.
fn a_controls_b() -> Pair {
    let mut pair = connected();
    push(&mut pair, Side::Server, 5000);
    assert_eq!(pair.server.phase(), Phase::Sending);
    assert_eq!(pair.client.phase(), Phase::Receiving);
    pair.advance(200);
    pair.clear_log();
    pair
}

fn reclaimed(pair: &Pair, side: Side, here: bool) -> bool {
    pair.notices(side)
        .iter()
        .any(|notice| matches!(notice, Notice::ControlReclaimed { here: h } if *h == here))
}

#[test]
fn the_other_computer_crosses_too() {
    // O que antes era impossível sem trocar o papel: o mouse de B vai para A pela esquerda de B.
    let mut pair = connected();
    push(&mut pair, Side::Client, -5000);

    assert_eq!(pair.client.phase(), Phase::Sending);
    assert_eq!(pair.server.phase(), Phase::Receiving);
    assert!(
        pair.any(Side::Client, is::suppress),
        "a entrada de B vai só para A"
    );

    pair.feed(
        Side::Client,
        Input::LocalKey {
            usage: A_KEY,
            pressed: true,
        },
    );
    assert!(
        pair.any(Side::Server, |c| matches!(
            c,
            Command::Inject(ir_session::Injection::Key { usage, pressed: true }) if *usage == A_KEY
        )),
        "a tecla de B é digitada em A"
    );
}

#[test]
fn and_the_way_back_is_the_opposite_edge_of_whoever_is_being_used() {
    let mut pair = connected();
    push(&mut pair, Side::Client, -5000);
    // B segue mandando: o ponteiro anda em A, e volta pela direita de A.
    push(&mut pair, Side::Client, 5000);
    pair.advance(20);
    assert_eq!(pair.client.phase(), Phase::Ready);
    assert_eq!(pair.server.phase(), Phase::Ready);
}

#[test]
fn moving_the_mouse_here_takes_control_back() {
    let mut pair = a_controls_b();
    push(&mut pair, Side::Client, RECLAIM_DISTANCE + 5);

    assert_eq!(
        pair.client.phase(),
        Phase::Ready,
        "B voltou a usar a própria tela"
    );
    assert_eq!(pair.server.phase(), Phase::Ready, "A parou de mandar");
    assert!(
        pair.any(Side::Server, is::unsuppress),
        "e o mouse de A volta a ser de A"
    );
    assert!(reclaimed(&pair, Side::Client, true));
    assert!(reclaimed(&pair, Side::Server, false));
    // Os avisos são espelhados: o controle mudou de lado nas duas pontas, e foi por retomada.
    for side in [Side::Client, Side::Server] {
        assert!(
            pair.notices(side)
                .contains(&Notice::ControlMoved { remote: false }),
            "{side:?}: o controle está nesta tela"
        );
    }
}

#[test]
fn a_bump_on_the_desk_does_not() {
    let mut pair = a_controls_b();
    push(&mut pair, Side::Client, 3);
    pair.advance(400);
    push(&mut pair, Side::Client, 3);
    assert_eq!(pair.client.phase(), Phase::Receiving, "tremida não é gesto");
    assert_eq!(pair.server.phase(), Phase::Sending);
}

#[test]
fn nothing_takes_it_back_while_the_hand_that_crossed_is_arriving() {
    let mut pair = connected();
    push(&mut pair, Side::Server, 5000);
    // No mesmo instante em que A chegou, B mexe muito — ainda é a mão de A chegando.
    push(&mut pair, Side::Client, 500);
    pair.feed(
        Side::Client,
        Input::LocalButton {
            button: Button::Left,
            pressed: true,
        },
    );
    assert_eq!(pair.client.phase(), Phase::Receiving);
}

#[test]
fn a_key_or_a_click_here_takes_it_back_and_what_the_other_held_is_released() {
    let mut pair = a_controls_b();
    pair.feed(
        Side::Server,
        Input::LocalKey {
            usage: A_KEY,
            pressed: true,
        },
    );
    assert!(
        !pair.client.input_state().is_released(),
        "A segura uma tecla em B"
    );
    pair.clear_log();

    pair.feed(
        Side::Client,
        Input::LocalButton {
            button: Button::Left,
            pressed: true,
        },
    );

    assert_eq!(pair.client.phase(), Phase::Ready);
    assert!(
        pair.any(Side::Client, is::release_all),
        "a tecla de A não fica presa em B"
    );
    assert!(pair.client.input_state().is_released());
}

#[test]
fn a_modifier_alone_is_not_a_gesture() {
    let mut pair = a_controls_b();
    pair.feed(
        Side::Client,
        Input::LocalKey {
            usage: HidUsage(0xE0),
            pressed: true,
        },
    );
    assert_eq!(pair.client.phase(), Phase::Receiving);
}

#[test]
fn releasing_a_key_is_not_a_gesture() {
    // Uma tecla apertada em B antes de A chegar, solta agora, não é alguém querendo usar B.
    let mut pair = a_controls_b();
    pair.feed(
        Side::Client,
        Input::LocalKey {
            usage: A_KEY,
            pressed: false,
        },
    );
    assert_eq!(pair.client.phase(), Phase::Receiving);
}

#[test]
fn the_switch_shortcut_here_takes_control_back() {
    let mut pair = a_controls_b();
    for usage in [0xE0, 0xE2, 0xE1] {
        pair.feed(
            Side::Client,
            Input::LocalKey {
                usage: HidUsage(usage),
                pressed: true,
            },
        );
    }
    // Os modificadores sozinhos não retomam: se o Ctrl retomasse, o resto do atalho levaria o
    // controle de volta para A.
    pair.feed(
        Side::Client,
        Input::LocalKey {
            usage: HidUsage(0x2C),
            pressed: true,
        },
    );
    assert_eq!(pair.client.phase(), Phase::Ready);
    assert_eq!(pair.server.phase(), Phase::Ready);
}

#[test]
fn when_both_cross_at_once_exactly_one_is_in_control() {
    let mut pair = connected();
    pair.set_delivery(false);
    push(&mut pair, Side::Server, 5000);
    push(&mut pair, Side::Client, -5000);
    assert_eq!(pair.server.phase(), Phase::Sending);
    assert_eq!(pair.client.phase(), Phase::Sending);

    pair.set_delivery(true);
    for _ in 0..10 {
        pair.advance(30);
    }

    assert_eq!(
        (pair.server.phase(), pair.client.phase()),
        (Phase::Sending, Phase::Receiving),
        "cede quem tem o identificador maior"
    );
    assert!(
        pair.any(Side::Client, is::unsuppress),
        "quem cedeu tem a entrada de volta"
    );
    assert!(
        pair.any(Side::Client, is::release_all),
        "pela mesma liberação de toda devolução"
    );
}

#[test]
fn a_computer_that_is_never_controlled_is_a_wall_for_the_other() {
    let mut pair = with_policies(Policy::Both, Policy::OnlyControls);
    push(&mut pair, Side::Server, 5000);
    assert_eq!(
        pair.server.phase(),
        Phase::Ready,
        "B não aceita: a borda de A é parede"
    );
    assert_eq!(pair.client.phase(), Phase::Ready);

    // Mas B controla A.
    push(&mut pair, Side::Client, -5000);
    assert_eq!(pair.client.phase(), Phase::Sending);
    assert_eq!(pair.server.phase(), Phase::Receiving);
}

#[test]
fn a_computer_that_never_controls_does_not_cross() {
    let mut pair = with_policies(Policy::OnlyControlled, Policy::Both);
    push(&mut pair, Side::Server, 5000);
    assert_eq!(pair.server.phase(), Phase::Ready);

    // O switch também não leva.
    for usage in [0xE0, 0xE2, 0xE1, 0x2C] {
        pair.feed(
            Side::Server,
            Input::LocalKey {
                usage: HidUsage(usage),
                pressed: true,
            },
        );
    }
    assert_eq!(pair.server.phase(), Phase::Ready);
}

#[test]
fn losing_the_link_while_being_used_releases_what_the_other_held() {
    let mut pair = a_controls_b();
    pair.feed(
        Side::Server,
        Input::LocalKey {
            usage: A_KEY,
            pressed: true,
        },
    );
    pair.set_delivery(false);
    pair.advance(1100);
    assert_eq!(pair.client.phase(), Phase::Offline);
    assert!(pair.client.input_state().is_released());
    assert!(pair.any(Side::Client, is::release_all));
}

#[test]
fn a_locked_computer_that_refuses_is_a_wall_until_it_unlocks() {
    // O relato do log 52: o Linux na tela de bloqueio, sem a permissão, e o cursor do Windows
    // atravessava para ficar preso lá, sem efeito nenhum.
    let mut pair = connected();
    pair.feed(Side::Client, Input::LocalProtectedDesktop(true));
    push(&mut pair, Side::Server, 5000);
    assert_eq!(pair.server.phase(), Phase::Ready, "a borda é parede");
    assert_eq!(pair.client.phase(), Phase::Ready);

    pair.feed(Side::Client, Input::LocalProtectedDesktop(false));
    push(&mut pair, Side::Server, 5000);
    assert_eq!(
        pair.server.phase(),
        Phase::Sending,
        "desbloqueado, atravessa de novo"
    );
}

#[test]
fn locking_while_being_used_sends_the_cursor_home() {
    let mut pair = a_controls_b();
    pair.feed(Side::Client, Input::LocalProtectedDesktop(true));
    assert_eq!(pair.client.phase(), Phase::Ready);
    assert_eq!(
        pair.server.phase(),
        Phase::Ready,
        "A não fica mandando ao nada"
    );
    assert!(
        pair.any(Side::Server, is::unsuppress),
        "e o mouse de A volta a ser de A"
    );
}

#[test]
fn a_computer_that_reconnects_while_locked_still_refuses() {
    let mut pair = connected();
    pair.feed(Side::Client, Input::LocalProtectedDesktop(true));
    pair.set_delivery(false);
    pair.advance(1100);
    assert_eq!(pair.server.phase(), Phase::Offline);
    pair.set_delivery(true);
    pair.connect(Carrier::Udp);
    push(&mut pair, Side::Server, 5000);
    assert_eq!(
        pair.server.phase(),
        Phase::Ready,
        "a sessão nova também sabe"
    );
}
