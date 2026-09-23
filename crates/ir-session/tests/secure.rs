//! Os atalhos de quem tem o teclado e o desktop protegido do controlado (protocolo 4).
//!
//! Ctrl+Alt+End pede o Ctrl+Alt+Del do outro lado; Ctrl+Alt+Shift+Esc é a emergência; e o
//! controlado conta quando recusa digitação na tela de bloqueio.
#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

mod common;

use common::{Pair, Side, is};
use ir_proto::carrier::Carrier;
use ir_proto::input::{HidUsage, PointerDelta};
use ir_session::event::Notice;
use ir_session::{Command, Input, Phase};

const CTRL: HidUsage = HidUsage(0xE0);
const ALT: HidUsage = HidUsage(0xE2);
const SHIFT: HidUsage = HidUsage(0xE1);
const END: HidUsage = HidUsage(0x4D);
const ESC: HidUsage = HidUsage(0x29);

fn controlling() -> Pair {
    let mut pair = Pair::matched();
    pair.connect(Carrier::Udp);
    pair.feed(
        Side::Server,
        Input::LocalPointer(PointerDelta { dx: 5000, dy: 0 }),
    );
    assert_eq!(pair.server.phase(), Phase::Engaged);
    pair.clear_log();
    pair
}

fn key(pair: &mut Pair, usage: HidUsage, pressed: bool) {
    pair.feed(Side::Server, Input::LocalKey { usage, pressed });
}

fn injected_key(pair: &Pair, usage: HidUsage) -> bool {
    pair.any(Side::Client, |c| {
        matches!(c, Command::Inject(ir_session::Injection::Key { usage: u, .. }) if *u == usage)
    })
}

#[test]
fn ctrl_alt_end_becomes_ctrl_alt_del_on_the_peer_and_end_never_arrives() {
    let mut pair = controlling();
    key(&mut pair, CTRL, true);
    key(&mut pair, ALT, true);
    key(&mut pair, END, true);
    key(&mut pair, END, false);

    assert!(
        pair.any(Side::Client, |c| matches!(c, Command::SecureAttention)),
        "o controlado recebe o pedido de Ctrl+Alt+Del"
    );
    assert!(
        !injected_key(&pair, END),
        "a tecla do atalho não vai ao par"
    );
    assert!(
        injected_key(&pair, CTRL),
        "os modificadores vão, como sempre"
    );
}

#[test]
fn ctrl_alt_shift_esc_gives_control_back_and_releases_the_peer() {
    let mut pair = controlling();
    key(&mut pair, CTRL, true);
    key(&mut pair, ALT, true);
    key(&mut pair, SHIFT, true);
    key(&mut pair, ESC, true);

    assert_eq!(pair.server.phase(), Phase::Ready, "o controle voltou");
    assert!(
        pair.client.input_state().is_released(),
        "e nada ficou preso lá"
    );
    assert!(pair.any(Side::Client, is::release_all));
    assert!(!injected_key(&pair, ESC));
}

#[test]
fn with_control_here_the_shortcut_is_just_keys() {
    let mut pair = Pair::matched();
    pair.connect(Carrier::Udp);
    pair.clear_log();
    key(&mut pair, CTRL, true);
    key(&mut pair, ALT, true);
    key(&mut pair, END, true);
    assert!(!pair.any(Side::Client, |c| matches!(c, Command::SecureAttention)));
}

#[test]
fn the_button_also_asks_for_ctrl_alt_del() {
    let mut pair = Pair::matched();
    pair.connect(Carrier::Udp);
    pair.clear_log();
    pair.feed(Side::Server, Input::SecureAttention);
    assert!(pair.any(Side::Client, |c| matches!(c, Command::SecureAttention)));
}

#[test]
fn without_a_session_the_request_says_it_could_not_go() {
    let mut pair = Pair::matched();
    pair.feed(Side::Server, Input::SecureAttention);
    assert!(
        pair.notices(Side::Server)
            .contains(&Notice::PeerCannotSecureAttention)
    );
}

#[test]
fn the_controlling_side_learns_the_peer_refuses_the_lock_screen() {
    let mut pair = controlling();
    pair.feed(Side::Client, Input::LocalProtectedDesktop(true));
    assert!(
        pair.notices(Side::Server)
            .contains(&Notice::PeerProtectedDesktop { refused: true })
    );
    pair.feed(Side::Client, Input::LocalProtectedDesktop(false));
    assert!(
        pair.notices(Side::Server)
            .contains(&Notice::PeerProtectedDesktop { refused: false })
    );
}

const SPACE: HidUsage = HidUsage(0x2C);

#[test]
fn ctrl_alt_shift_space_goes_to_the_peer_and_back() {
    let mut pair = Pair::matched();
    pair.connect(Carrier::Udp);
    key(&mut pair, CTRL, true);
    key(&mut pair, ALT, true);
    key(&mut pair, SHIFT, true);
    key(&mut pair, SPACE, true);
    assert_eq!(
        pair.server.phase(),
        Phase::Engaged,
        "foi sem passar pela borda"
    );
    key(&mut pair, SPACE, false);

    key(&mut pair, SPACE, true);
    assert_eq!(pair.server.phase(), Phase::Ready, "e voltou");
    assert!(pair.client.input_state().is_released(), "soltando tudo lá");
}

#[test]
fn with_the_edge_locked_the_pointer_stays_here() {
    let mut pair = Pair::matched();
    pair.connect(Carrier::Udp);
    pair.feed(Side::Server, Input::LockEdge(true));
    pair.feed(
        Side::Server,
        Input::LocalPointer(PointerDelta { dx: 5000, dy: 0 }),
    );
    assert_eq!(
        pair.server.phase(),
        Phase::Ready,
        "a borda travada não atravessa"
    );

    pair.feed(Side::Server, Input::LockEdge(false));
    pair.feed(
        Side::Server,
        Input::LocalPointer(PointerDelta { dx: 5000, dy: 0 }),
    );
    assert_eq!(pair.server.phase(), Phase::Engaged);
}

#[test]
fn locking_here_asks_the_controlled_peer_to_lock() {
    let mut pair = Pair::matched();
    pair.connect(Carrier::Udp);
    pair.clear_log();
    pair.feed(Side::Server, Input::LockPeerScreen);
    assert!(pair.any(Side::Client, |c| matches!(c, Command::LockScreen)));
}
