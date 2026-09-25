//! Cenários de estado de entrada: encaminhamento, snapshot, reconciliação e emergência.
//!
//! Linhas correspondentes de `docs/10-testes-e-validacao.md` §2: enlace que cai com teclas
//! pressionadas, snapshot divergente, agente perdido e o atalho de emergência.
//!
//! É o grupo mais importante do produto: todo cenário aqui existe para provar que **nada
//! fica pressionado**.
#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

mod common;

use common::{Pair, Side, is};
use ir_proto::carrier::Carrier;
use ir_proto::input::{Button, HidUsage, PointerDelta};
use ir_session::event::{LinkDown, Notice};
use ir_session::{Command, Input, Phase};

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
fn keys_typed_while_remote_reach_the_client_and_come_back_released() {
    let mut pair = connected();
    cross_to_client(&mut pair);
    pair.clear_log();

    pair.feed(
        Side::Server,
        Input::LocalKey {
            usage: HidUsage(0x04),
            pressed: true,
        },
    );
    assert!(
        pair.any(Side::Client, is::injection),
        "a tecla precisa ser injetada"
    );
    assert!(pair.client.input_state().keys.contains(HidUsage(0x04)));

    pair.feed(
        Side::Server,
        Input::LocalKey {
            usage: HidUsage(0x04),
            pressed: false,
        },
    );
    assert!(
        pair.client.input_state().is_released(),
        "e precisa ser solta"
    );
}

#[test]
fn a_link_drop_with_three_keys_held_releases_before_anything_else() {
    let mut pair = connected();
    cross_to_client(&mut pair);
    for usage in [0x04u16, 0x05, 0x06] {
        pair.feed(
            Side::Server,
            Input::LocalKey {
                usage: HidUsage(usage),
                pressed: true,
            },
        );
    }
    assert_eq!(pair.client.input_state().keys.len(), 3);
    pair.clear_log();

    pair.feed(
        Side::Client,
        Input::CarrierDown {
            carrier: Carrier::Udp,
            reason: LinkDown::TransportFailed,
        },
    );

    assert!(
        pair.client.input_state().is_released(),
        "as três teclas precisam ser soltas"
    );
    let release = pair
        .index_of_release(Side::Client)
        .expect("ReleaseAll é obrigatório");
    let notice = pair
        .index_of(Side::Client, |c| matches!(c, Command::Notify(_)))
        .expect("a interface precisa ser avisada");
    assert!(
        release < notice,
        "soltar vem antes de qualquer outra coisa; a ordem é contrato"
    );
}

#[test]
fn losing_the_agent_while_being_controlled_gives_control_back() {
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

    pair.feed(Side::Client, Input::AgentLost);

    assert!(
        pair.any(Side::Client, is::release_all),
        "sem agente, solta tudo"
    );
    assert!(pair.client.input_state().is_released());
    assert_eq!(
        pair.server.phase(),
        Phase::Ready,
        "e o controle volta para quem digita"
    );
    // Como toda devolução de quem recebe que não é pela borda: uma retomada. O cursor de quem
    // digita fica onde saiu, sem ser levado de volta pela borda.
    let reclaimed = |side, here| {
        pair.notices(side)
            .contains(&Notice::ControlReclaimed { here })
    };
    assert!(reclaimed(Side::Client, true) && reclaimed(Side::Server, false));
    assert!(!pair.any(Side::Server, is::warp));
}

#[test]
fn losing_the_agent_while_controlling_releases_the_peer_and_takes_control_back() {
    let mut pair = connected();
    cross_to_client(&mut pair);
    pair.feed(
        Side::Server,
        Input::LocalKey {
            usage: HidUsage(0x04),
            pressed: true,
        },
    );
    assert!(
        !pair.client.input_state().is_released(),
        "a tecla desceu no par"
    );
    pair.clear_log();

    pair.feed(Side::Server, Input::AgentLost);

    assert!(
        pair.client.input_state().is_released(),
        "a subida nunca viria: o par solta tudo"
    );
    assert_eq!(
        pair.server.phase(),
        Phase::Ready,
        "e o controle volta para cá"
    );
}

#[test]
fn the_periodic_snapshot_is_sent_while_the_control_is_remote() {
    let mut pair = connected();
    cross_to_client(&mut pair);
    pair.clear_log();

    pair.advance(300);

    assert!(
        pair.any(Side::Server, is::snapshot),
        "o snapshot é a rede de segurança"
    );
}

#[test]
fn no_snapshot_is_sent_while_the_control_is_local() {
    let mut pair = connected();
    pair.clear_log();
    pair.advance(300);
    assert!(
        !pair.any(Side::Server, is::snapshot),
        "sem controle remoto não há o que sincronizar"
    );
}

#[test]
fn a_divergent_snapshot_is_reconciled_and_reconciling_again_does_nothing() {
    let mut pair = connected();
    cross_to_client(&mut pair);
    pair.feed(
        Side::Server,
        Input::LocalKey {
            usage: HidUsage(0x04),
            pressed: true,
        },
    );

    // O cliente ficou com uma tecla a mais, como se um `KeyUp` tivesse se perdido.
    pair.feed(Side::Client, Input::Tick);
    pair.clear_log();
    pair.advance(300); // dispara o snapshot do servidor

    assert!(
        pair.client.input_state().keys.contains(HidUsage(0x04)),
        "o estado bate"
    );

    // Um segundo snapshot idêntico não pode produzir injeção nenhuma.
    pair.clear_log();
    pair.advance(300);
    let injections = pair.count(Side::Client, is::injection);
    assert_eq!(
        injections, 0,
        "reconciliar de novo com o mesmo alvo não faz nada"
    );
}

#[test]
fn the_emergency_shortcut_returns_control_even_with_a_healthy_link() {
    let mut pair = connected();
    cross_to_client(&mut pair);
    pair.feed(
        Side::Server,
        Input::LocalKey {
            usage: HidUsage::LEFT_CTRL,
            pressed: true,
        },
    );
    pair.clear_log();

    pair.feed(Side::Server, Input::EmergencyRelease);

    assert_eq!(pair.server.phase(), Phase::Ready);
    assert!(pair.any(Side::Server, is::release_all));
    assert!(pair.any(Side::Server, is::unsuppress));
    assert!(pair.server.input_state().is_released());
    assert!(
        pair.client.input_state().is_released(),
        "o outro lado também precisa soltar"
    );
}

#[test]
fn the_emergency_shortcut_on_the_client_takes_control_back_too() {
    let mut pair = connected();
    cross_to_client(&mut pair);
    pair.clear_log();

    pair.feed(Side::Client, Input::EmergencyRelease);

    assert_eq!(pair.client.phase(), Phase::Ready);
    assert_eq!(
        pair.server.phase(),
        Phase::Ready,
        "o servidor precisa retomar"
    );
    assert!(pair.client.input_state().is_released());
}

#[test]
fn buttons_are_forwarded_and_released_like_keys() {
    let mut pair = connected();
    cross_to_client(&mut pair);
    pair.clear_log();

    pair.feed(
        Side::Server,
        Input::LocalButton {
            button: Button::Left,
            pressed: true,
        },
    );
    assert!(pair.client.input_state().buttons.contains(Button::Left));

    pair.feed(
        Side::Server,
        Input::LocalButton {
            button: Button::Left,
            pressed: false,
        },
    );
    assert!(pair.client.input_state().is_released());
}

#[test]
fn nothing_is_forwarded_while_the_control_is_local() {
    let mut pair = connected();
    pair.clear_log();

    pair.feed(
        Side::Server,
        Input::LocalKey {
            usage: HidUsage(0x04),
            pressed: true,
        },
    );

    assert!(
        !pair.any(Side::Client, is::injection),
        "o cliente não pode receber nada"
    );
    assert!(
        pair.server.input_state().is_released(),
        "e o servidor não guarda estado remoto"
    );
}
