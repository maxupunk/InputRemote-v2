//! O lado emissor da confiabilidade sobre datagrama: janela, confirmação, retransmissão e
//! desistência.
//!
//! Separado do receptor (`tests/reliability.rs`) porque são duas responsabilidades, e porque é aqui
//! que mora a regra que decide se um pico de latência vira atraso ou queda: a espera entre reenvios
//! dobra, e o enlace só cai quando uma mensagem passa do prazo sem confirmação, contado do primeiro
//! envio (log 23).

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use ir_proto::frame::{Ack, Frame, Sequence};
use ir_proto::input::{HidUsage, Modifiers};
use ir_proto::message::{InputMessage, Message};
use ir_session::reliability::{SendOutcome, Sender, TimeoutOutcome, WINDOW};
use ir_session::{Millis, Timestamp};

const FLOOR: Millis = Millis(20);
const CEILING: Millis = Millis(1000);

fn frame(seq: u32) -> Frame {
    Frame::new(
        Message::Input(InputMessage::KeyDown {
            usage: HidUsage(0x04),
            mods: Modifiers::NONE,
        }),
        Sequence(seq),
    )
}

fn at(millis: u64) -> Timestamp {
    Timestamp::from_millis(millis)
}

#[test]
fn a_fresh_sender_has_nothing_pending() {
    let sender = Sender::new();
    assert!(sender.is_idle());
    assert_eq!(sender.pending(), 0);
}

#[test]
fn an_acknowledged_message_stops_being_pending() {
    let mut sender = Sender::new();
    assert_eq!(
        sender.on_sent(at(0), Sequence(1), frame(1)),
        SendOutcome::Accepted
    );
    assert_eq!(sender.pending(), 1);

    sender.on_ack(at(10), Ack::new(Sequence(1)));
    assert!(sender.is_idle(), "confirmado deixa de ser pendente");
}

#[test]
fn an_unrelated_ack_does_not_clear_the_window() {
    let mut sender = Sender::new();
    sender.on_sent(at(0), Sequence(1), frame(1));
    sender.on_ack(at(10), Ack::new(Sequence(99)));
    assert_eq!(
        sender.pending(),
        1,
        "confirmação de outra sequência não conta"
    );
}

#[test]
fn a_cumulative_ack_clears_everything_it_covers() {
    let mut sender = Sender::new();
    for seq in 1..=3 {
        sender.on_sent(at(0), Sequence(seq), frame(seq));
    }
    // Cumulativa em 3, com 2 e 1 marcadas no bitmap.
    let ack = Ack::new(Sequence(3)).with(Sequence(2)).with(Sequence(1));
    sender.on_ack(at(10), ack);
    assert!(sender.is_idle());
}

#[test]
fn the_window_refuses_to_grow_and_the_caller_must_drop_the_link() {
    let mut sender = Sender::new();
    for seq in 0..u32::try_from(WINDOW).unwrap() {
        assert_eq!(
            sender.on_sent(at(0), Sequence(seq), frame(seq)),
            SendOutcome::Accepted
        );
    }
    assert_eq!(
        sender.on_sent(at(0), Sequence(999), frame(999)),
        SendOutcome::WindowFull,
        "descartar em silêncio perderia um evento de teclado"
    );
    assert_eq!(sender.pending(), WINDOW, "e a janela não cresce");
}

#[test]
fn nothing_is_retransmitted_before_the_deadline() {
    let mut sender = Sender::new();
    sender.on_sent(at(0), Sequence(1), frame(1));
    assert_eq!(
        sender.on_tick(at(19), FLOOR, CEILING),
        TimeoutOutcome::Idle,
        "19 ms é menos que o piso de 20"
    );
}

#[test]
fn the_deadline_triggers_a_retransmission() {
    let mut sender = Sender::new();
    sender.on_sent(at(0), Sequence(1), frame(1));
    match sender.on_tick(at(20), FLOOR, CEILING) {
        TimeoutOutcome::Retransmit(frames) => {
            assert_eq!(frames.len(), 1);
            assert_eq!(frames.first().map(|f| f.seq), Some(Sequence(1)));
        }
        other => panic!("deveria retransmitir, deu {other:?}"),
    }
}

#[test]
fn a_retransmitted_message_waits_the_deadline_again() {
    let mut sender = Sender::new();
    sender.on_sent(at(0), Sequence(1), frame(1));
    let _ = sender.on_tick(at(20), FLOOR, CEILING);
    assert_eq!(
        sender.on_tick(at(30), FLOOR, CEILING),
        TimeoutOutcome::Idle,
        "o prazo reconta a partir do reenvio"
    );
}

#[test]
fn the_link_is_dropped_only_when_a_message_outlives_the_deadline() {
    // Desistir é por tempo, contado do primeiro envio, e não por número de tentativas. Contar
    // tentativas com prazo de 20 ms desistia em ~100 ms, e um pico de 124 ms do Wi-Fi com
    // economia de energia virava "o par morreu" (log 23). Esperar mais não cria lacuna: a
    // mensagem continua na fila, em ordem; só chega mais tarde.
    let deadline = u64::from(CEILING.get());
    let mut sender = Sender::new();
    sender.on_sent(at(0), Sequence(7), frame(7));

    let mut now = 0u64;
    while now + 10 < deadline {
        now += 10;
        assert!(
            !matches!(
                sender.on_tick(at(now), FLOOR, CEILING),
                TimeoutOutcome::GiveUp { .. }
            ),
            "desistiu aos {now} ms, antes do prazo de {deadline} ms"
        );
    }

    match sender.on_tick(at(deadline), FLOOR, CEILING) {
        TimeoutOutcome::GiveUp { seq } => assert_eq!(
            seq,
            Sequence(7),
            "o diagnóstico precisa dizer qual sequência morreu"
        ),
        other => panic!(
            "passado o prazo sem confirmação, o enlace tinha de cair, senão seguiria com lacuna; deu {other:?}"
        ),
    }
}

#[test]
fn a_stall_shorter_than_the_deadline_is_waited_out() {
    let mut sender = Sender::new();
    sender.on_sent(at(0), Sequence(1), frame(1));
    for now in (10..=400).step_by(10) {
        assert!(
            !matches!(
                sender.on_tick(at(now), FLOOR, CEILING),
                TimeoutOutcome::GiveUp { .. }
            ),
            "um silêncio de {now} ms não é o par morto"
        );
    }
    sender.on_ack(at(410), Ack::new(Sequence(1)));
    assert!(
        sender.is_idle(),
        "a confirmação que chega depois do pico encerra a espera"
    );
}

#[test]
fn retransmissions_back_off_instead_of_hammering_the_link() {
    // Sem crescimento, as tentativas cabiam dentro de um pico só. Dobrando, elas se espalham:
    // 20, 40, 80 e 160 ms, e depois 250 ms — um quarto do prazo — até ele vencer. Parar num
    // número fixo de envios deixaria sem reenvio a mensagem que se perdeu inteira num pico.
    let mut sender = Sender::new();
    sender.on_sent(at(0), Sequence(1), frame(1));
    let mut resent_at = Vec::new();
    for now in 1..=900u64 {
        if let TimeoutOutcome::Retransmit(_) = sender.on_tick(at(now), FLOOR, CEILING) {
            resent_at.push(now);
        }
    }
    assert_eq!(
        resent_at,
        vec![20, 60, 140, 300, 550, 800],
        "os reenvios precisam dobrar até o teto de um quarto do prazo, e continuar até ele"
    );
}

#[test]
fn the_retransmit_deadline_follows_the_measured_round_trip() {
    let mut sender = Sender::new();
    assert_eq!(
        sender.retransmit_after(FLOOR, CEILING),
        FLOOR,
        "sem amostra, usa o piso"
    );

    // Uma ida e volta de 200 ms: o prazo passa a ser o dobro.
    sender.on_sent(at(0), Sequence(1), frame(1));
    sender.on_ack(at(200), Ack::new(Sequence(1)));
    assert_eq!(sender.retransmit_after(FLOOR, CEILING), Millis(400));
}

#[test]
fn the_retransmit_deadline_never_passes_the_link_timeout() {
    let mut sender = Sender::new();
    sender.on_sent(at(0), Sequence(1), frame(1));
    sender.on_ack(at(5000), Ack::new(Sequence(1)));
    assert_eq!(
        sender.retransmit_after(FLOOR, CEILING),
        CEILING,
        "retransmitir depois de o enlace ser declarado morto não serve"
    );
}

#[test]
fn a_retransmitted_message_does_not_pollute_the_round_trip_estimate() {
    let mut sender = Sender::new();
    sender.on_sent(at(0), Sequence(1), frame(1));
    let _ = sender.on_tick(at(20), FLOOR, CEILING);
    sender.on_ack(at(25), Ack::new(Sequence(1)));
    assert_eq!(
        sender.retransmit_after(FLOOR, CEILING),
        FLOOR,
        "o tempo de uma retransmissão mede o prazo, não o enlace"
    );
}

#[test]
fn resetting_clears_the_window_and_the_estimate() {
    let mut sender = Sender::new();
    sender.on_sent(at(0), Sequence(1), frame(1));
    sender.on_ack(at(300), Ack::new(Sequence(1)));
    sender.on_sent(at(300), Sequence(2), frame(2));

    sender.reset();
    assert!(sender.is_idle());
    assert_eq!(
        sender.retransmit_after(FLOOR, CEILING),
        FLOOR,
        "a estimativa volta ao piso"
    );
}
