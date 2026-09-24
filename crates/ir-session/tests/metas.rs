//! As metas de engenharia de `docs/01-visao-e-escopo.md` §6 que a sessão sozinha consegue provar.
//!
//! Estavam escritas no escopo e não tinham teste nenhum. As que dependem de rádio e rede de
//! verdade (atraso do Bluetooth, reconexão em 5 s) continuam na bancada; estas são as que valem
//! para qualquer portador, porque são da máquina de estados:
//!
//! - o ponteiro chega ao par a pelo menos 125 Hz enquanto se move;
//! - zero tecla presa depois de 10 000 travessias com tecla segurada.
#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

mod common;

use common::{Pair, Side};
use ir_proto::carrier::Carrier;
use ir_proto::input::{HidUsage, PointerDelta};
use ir_proto::message::{Message, PointerMessage};
use ir_session::{Command, Input, Phase};

fn pointer_messages(pair: &Pair) -> usize {
    pair.count(Side::Server, |c| {
        matches!(
            c,
            Command::Send { frame, .. } if matches!(frame.message, Message::Pointer(PointerMessage::Motion { .. } | PointerMessage::Position { .. }))
        )
    })
}

#[test]
fn the_pointer_reaches_the_peer_at_125_hz_or_more() {
    let mut pair = Pair::matched();
    pair.connect(Carrier::Udp);
    pair.feed(
        Side::Server,
        Input::LocalPointer(PointerDelta { dx: 5000, dy: 0 }),
    );
    assert_eq!(pair.server.phase(), Phase::Sending);
    pair.clear_log();

    // Um mouse de 1 kHz, andando em círculo pequeno para não sair pela borda, por um segundo.
    for milissegundo in 0..1000 {
        let dx = if (milissegundo / 50) % 2 == 0 { 2 } else { -2 };
        pair.feed(
            Side::Server,
            Input::LocalPointer(PointerDelta { dx, dy: 0 }),
        );
        pair.advance(1);
    }

    let enviados = pointer_messages(&pair);
    assert!(
        enviados >= 125,
        "o ponteiro saiu só {enviados} vezes em 1 s; a meta é 125 Hz"
    );
    assert!(
        enviados <= 1000,
        "coalescido: nunca mais quadros que eventos ({enviados})"
    );
}

#[test]
fn ten_thousand_crossings_with_a_key_held_leave_nothing_pressed() {
    let mut pair = Pair::matched();
    pair.connect(Carrier::Udp);
    let tecla = HidUsage(0x04);

    for volta in 0..10_000 {
        pair.feed(
            Side::Server,
            Input::LocalPointer(PointerDelta { dx: 5000, dy: 0 }),
        );
        pair.feed(
            Side::Server,
            Input::LocalKey {
                usage: tecla,
                pressed: true,
            },
        );
        // Volta segurando a tecla — o caso que prende tecla quando alguma coisa está errada.
        pair.feed(
            Side::Server,
            Input::LocalPointer(PointerDelta { dx: -5000, dy: 0 }),
        );
        pair.advance(20);
        pair.feed(
            Side::Server,
            Input::LocalKey {
                usage: tecla,
                pressed: false,
            },
        );
        assert!(
            pair.client.input_state().is_released(),
            "tecla presa no par na travessia {volta}"
        );
        if volta % 500 == 0 {
            pair.clear_log(); // o registro dos comandos não precisa guardar 10 mil voltas
        }
    }
    assert_eq!(pair.server.phase(), Phase::Ready);
}
