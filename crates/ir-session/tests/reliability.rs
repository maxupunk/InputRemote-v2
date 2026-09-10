//! A camada de confiabilidade sobre datagrama.
//!
//! Cenários de `docs/10-testes-e-validacao.md` §5: perda injetada não produz tecla presa, e a
//! sessão cai em vez de prosseguir com lacuna.

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use ir_proto::frame::{Ack, Frame, Sequence};
use ir_proto::input::{HidUsage, Modifiers};
use ir_proto::message::{InputMessage, Message};
use ir_session::reliability::{Delivery, Receiver, SendOutcome, Sender, TimeoutOutcome, WINDOW};
use ir_session::{Millis, Timestamp};

const FLOOR: Millis = Millis(20);
const CEILING: Millis = Millis(1000);
const MAX_TRIES: u8 = 5;

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
        sender.on_tick(at(19), FLOOR, CEILING, MAX_TRIES),
        TimeoutOutcome::Idle,
        "19 ms é menos que o piso de 20"
    );
}

#[test]
fn the_deadline_triggers_a_retransmission() {
    let mut sender = Sender::new();
    sender.on_sent(at(0), Sequence(1), frame(1));
    match sender.on_tick(at(20), FLOOR, CEILING, MAX_TRIES) {
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
    let _ = sender.on_tick(at(20), FLOOR, CEILING, MAX_TRIES);
    assert_eq!(
        sender.on_tick(at(30), FLOOR, CEILING, MAX_TRIES),
        TimeoutOutcome::Idle,
        "o prazo reconta a partir do reenvio"
    );
}

#[test]
fn the_link_is_dropped_after_the_last_attempt_instead_of_going_on() {
    let mut sender = Sender::new();
    sender.on_sent(at(0), Sequence(7), frame(7));

    let mut now = 0u64;
    for _ in 0..MAX_TRIES {
        now += 20;
        if let TimeoutOutcome::GiveUp { seq } = sender.on_tick(at(now), FLOOR, CEILING, MAX_TRIES) {
            assert_eq!(
                seq,
                Sequence(7),
                "o diagnóstico precisa dizer qual sequência morreu"
            );
            return;
        }
    }
    panic!("esgotadas as tentativas, o enlace tinha de cair — prosseguir deixaria tecla presa");
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
    let _ = sender.on_tick(at(20), FLOOR, CEILING, MAX_TRIES);
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

/// Entrega um quadro e devolve as sequências que saíram, na ordem.
fn deliver(receiver: &mut Receiver, seq: u32) -> Vec<u32> {
    match receiver.accept(Sequence(seq), frame(seq)) {
        Delivery::Ready(frames) => frames.iter().map(|f| f.seq.get()).collect(),
        _ => Vec::new(),
    }
}

#[test]
fn the_first_frame_defines_the_starting_point() {
    let mut receiver = Receiver::new();
    assert_eq!(
        deliver(&mut receiver, 42),
        vec![42],
        "não há como saber o que veio antes"
    );
    assert_eq!(
        receiver.ack_to_send().map(|a| a.cumulative),
        Some(Sequence(42))
    );
}

#[test]
fn a_repeated_frame_is_refused() {
    let mut receiver = Receiver::new();
    receiver.accept(Sequence(1), frame(1));
    assert_eq!(
        receiver.accept(Sequence(1), frame(1)),
        Delivery::Duplicate,
        "reaplicar um KeyDown gravado do ar seria redigitar o que o usuário digitou"
    );
}

#[test]
fn frames_in_order_are_delivered_one_by_one() {
    let mut receiver = Receiver::new();
    for seq in 1..=10 {
        assert_eq!(deliver(&mut receiver, seq), vec![seq], "seq {seq}");
    }
    assert_eq!(
        receiver.ack_to_send().map(|a| a.cumulative),
        Some(Sequence(10))
    );
    assert_eq!(receiver.buffered(), 0);
}

#[test]
fn a_frame_that_arrives_early_waits_for_the_gap_to_be_filled() {
    // O cenário exato que deixa tecla presa: o `KeyDown` (2) se perde, o `KeyUp` (3) chega
    // inteiro, e o `KeyDown` retransmitido chega depois. Entregar na ordem de chegada
    // pressionaria a tecla depois de o evento que a soltaria já ter passado.
    let mut receiver = Receiver::new();
    deliver(&mut receiver, 1);

    assert_eq!(receiver.accept(Sequence(3), frame(3)), Delivery::Buffered);
    assert_eq!(receiver.buffered(), 1, "o 3 espera o 2");

    assert_eq!(
        deliver(&mut receiver, 2),
        vec![2, 3],
        "o 2 destrava o 3, e nesta ordem"
    );
    assert_eq!(receiver.buffered(), 0);
}

#[test]
fn a_long_gap_releases_everything_in_order_when_it_closes() {
    let mut receiver = Receiver::new();
    deliver(&mut receiver, 0);
    for seq in [4u32, 2, 5, 3] {
        assert_eq!(
            receiver.accept(Sequence(seq), frame(seq)),
            Delivery::Buffered
        );
    }
    assert_eq!(receiver.buffered(), 4);
    assert_eq!(
        deliver(&mut receiver, 1),
        vec![1, 2, 3, 4, 5],
        "tudo, na ordem certa"
    );
}

#[test]
fn a_frame_older_than_what_was_delivered_is_refused_not_buffered() {
    let mut receiver = Receiver::new();
    for seq in 100..=105 {
        deliver(&mut receiver, seq);
    }
    assert_eq!(
        receiver.accept(Sequence(10), frame(10)),
        Delivery::Duplicate,
        "guardá-lo seria esperar para sempre por um buraco que já passou"
    );
    assert_eq!(receiver.buffered(), 0);
}

#[test]
fn the_reorder_queue_refuses_to_grow_without_end() {
    let mut receiver = Receiver::new();
    deliver(&mut receiver, 0);
    // Nunca chega o 1: tudo depois dele fica esperando.
    let mut overflowed = false;
    for seq in 2..500u32 {
        if receiver.accept(Sequence(seq), frame(seq)) == Delivery::Overflow {
            overflowed = true;
            break;
        }
    }
    assert!(
        overflowed,
        "perder mais do que a fila repara tem de derrubar o enlace"
    );
}

#[test]
fn the_receiver_works_across_the_sequence_wraparound() {
    let mut receiver = Receiver::new();
    assert_eq!(deliver(&mut receiver, u32::MAX - 1), vec![u32::MAX - 1]);
    assert_eq!(deliver(&mut receiver, u32::MAX), vec![u32::MAX]);
    assert_eq!(
        deliver(&mut receiver, 0),
        vec![0],
        "0 vem depois de u32::MAX"
    );
    assert_eq!(deliver(&mut receiver, 1), vec![1]);
    assert_eq!(receiver.accept(Sequence(0), frame(0)), Delivery::Duplicate);
}

#[test]
fn resetting_the_receiver_forgets_everything() {
    let mut receiver = Receiver::new();
    deliver(&mut receiver, 5);
    receiver.accept(Sequence(9), frame(9));
    receiver.reset();

    assert!(receiver.ack_to_send().is_none());
    assert_eq!(receiver.buffered(), 0);
    assert_eq!(deliver(&mut receiver, 5), vec![5], "depois do reset é nova");
}

#[test]
fn five_percent_loss_delivers_everything_exactly_once_and_in_order() {
    // O cenário de estresse de docs/10 §5, em miniatura e determinístico: uma em cada vinte
    // mensagens se perde. No fim, todas têm de ter sido entregues, uma vez cada, em ordem.
    let mut sender = Sender::new();
    let mut receiver = Receiver::new();
    let mut delivered = Vec::new();
    let mut now = 0u64;

    let hand_over = |receiver: &mut Receiver, frame: Frame, out: &mut Vec<u32>| {
        if let Delivery::Ready(frames) = receiver.accept(frame.seq, frame) {
            out.extend(frames.iter().map(|f| f.seq.get()));
        }
    };

    for seq in 0..200u32 {
        now += 5;
        assert_eq!(
            sender.on_sent(at(now), Sequence(seq), frame(seq)),
            SendOutcome::Accepted
        );

        if seq % 20 != 7 {
            hand_over(&mut receiver, frame(seq), &mut delivered);
        }
        if let Some(ack) = receiver.ack_to_send() {
            sender.on_ack(at(now), ack);
        }

        now += 25;
        if let TimeoutOutcome::Retransmit(frames) =
            sender.on_tick(at(now), FLOOR, CEILING, MAX_TRIES)
        {
            for resent in frames {
                hand_over(&mut receiver, resent, &mut delivered);
            }
            if let Some(ack) = receiver.ack_to_send() {
                sender.on_ack(at(now), ack);
            }
        }
    }

    let expected: Vec<u32> = (0..200).collect();
    assert_eq!(
        delivered, expected,
        "tudo, uma vez cada, e na ordem original"
    );
    assert!(sender.is_idle(), "e nada pode ficar pendente no fim");
}
