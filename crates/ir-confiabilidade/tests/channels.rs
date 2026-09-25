//! Os canais juntos: quais têm confiabilidade, em que ordem são varridos, e que um não atrasa o outro.

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

mod common;

use common::{at, frame};
use ir_confiabilidade::time::Millis;
use ir_confiabilidade::{Delivery, Due, ReliableChannels, SendOutcome};
use ir_proto::channel::ChannelId;
use ir_proto::frame::Sequence;

/// Se `order` tem exatamente os canais cobertos, cada um uma vez.
fn is_permutation_of_covered(order: [ChannelId; 4]) -> bool {
    ReliableChannels::COVERED
        .iter()
        .all(|channel| order.iter().filter(|c| *c == channel).count() == 1)
}

#[test]
fn the_pointer_and_bulk_channels_are_not_covered() {
    assert!(!ReliableChannels::covers(ChannelId::Pointer));
    assert!(!ReliableChannels::covers(ChannelId::Bulk));
    for channel in ReliableChannels::COVERED {
        assert!(
            ReliableChannels::covers(channel),
            "{channel} deveria ser coberto"
        );
    }
}

#[test]
fn every_covered_channel_has_its_own_window_and_no_other_has_one() {
    // A lista e o mapa de pares são duas coisas escritas à mão; esta é a prova de que concordam.
    for channel in ChannelId::ALL {
        let mut channels = ReliableChannels::new();
        channels.on_sent(channel, at(0), Sequence(1), &frame(1));
        assert_eq!(
            channels.pending(channel) == 1,
            ReliableChannels::covers(channel),
            "{channel}"
        );
    }
}

#[test]
fn each_priority_order_sweeps_every_covered_channel_exactly_once() {
    assert!(is_permutation_of_covered(
        ReliableChannels::RETRANSMIT_ORDER
    ));
    assert!(is_permutation_of_covered(ReliableChannels::ACK_ORDER));
    assert_eq!(
        ReliableChannels::ACK_ORDER.first(),
        Some(&ChannelId::ReliableInput),
        "a janela da entrada é a que enche na digitação contínua"
    );
}

#[test]
fn an_uncovered_channel_is_always_accepted_and_never_pending() {
    let mut channels = ReliableChannels::new();
    for channel in [ChannelId::Pointer, ChannelId::Bulk] {
        assert_eq!(
            channels.on_sent(channel, at(0), Sequence(1), &frame(1)),
            SendOutcome::Accepted
        );
        assert_eq!(channels.pending(channel), 0, "{channel} não usa janela");
        assert!(matches!(
            channels.accept(channel, frame(1)),
            Delivery::Ready(_)
        ));
        assert!(
            matches!(channels.accept(channel, frame(1)), Delivery::Ready(_)),
            "sem detecção de repetição"
        );
        assert!(channels.ack_for(channel).is_none());
    }
}

#[test]
fn the_channels_do_not_share_a_window() {
    let mut channels = ReliableChannels::new();
    channels.on_sent(ChannelId::ClipboardText, at(0), Sequence(1), &frame(1));
    assert_eq!(channels.pending(ChannelId::ClipboardText), 1);
    assert_eq!(
        channels.pending(ChannelId::ReliableInput),
        0,
        "a retransmissão de clipboard não pode atrasar um KeyUp"
    );
}

#[test]
fn a_duplicate_is_detected_per_channel() {
    let mut channels = ReliableChannels::new();
    assert!(matches!(
        channels.accept(ChannelId::ReliableInput, frame(1)),
        Delivery::Ready(_)
    ));
    assert_eq!(
        channels.accept(ChannelId::ReliableInput, frame(1)),
        Delivery::Duplicate
    );
    assert!(
        matches!(
            channels.accept(ChannelId::Control, frame(1)),
            Delivery::Ready(_)
        ),
        "a sequência 1 do controle é outra mensagem"
    );
}

#[test]
fn an_out_of_order_frame_waits_for_the_one_before_it() {
    // O cenário que deixa tecla presa: o `KeyDown` se perde, o `KeyUp` chega, e o
    // `KeyDown` retransmitido chega depois. Entregar na ordem de chegada pressionaria a
    // tecla e nunca mais a soltaria.
    let mut channels = ReliableChannels::new();
    assert!(matches!(
        channels.accept(ChannelId::ReliableInput, frame(1)),
        Delivery::Ready(_)
    ));
    assert_eq!(
        channels.accept(ChannelId::ReliableInput, frame(3)),
        Delivery::Buffered,
        "o 3 espera o 2"
    );
    assert_eq!(channels.buffered(ChannelId::ReliableInput), 1);

    match channels.accept(ChannelId::ReliableInput, frame(2)) {
        Delivery::Ready(frames) => {
            let order: Vec<u32> = frames.iter().map(|f| f.seq.get()).collect();
            assert_eq!(order, vec![2, 3], "o 2 destrava o 3, e nesta ordem");
        }
        other => panic!("deveria entregar os dois, deu {other:?}"),
    }
    assert_eq!(channels.buffered(ChannelId::ReliableInput), 0);
}

#[test]
fn giving_up_reports_which_channel_died() {
    let mut channels = ReliableChannels::new();
    channels.on_sent(ChannelId::ReliableInput, at(0), Sequence(9), &frame(9));
    let mut now = 0u64;
    // Até um pouco depois do prazo de 1 s: desistir é por tempo, não por tentativas.
    for _ in 0..60 {
        now += 20;
        if let Due::GiveUp { channel, seq } = channels.on_tick(at(now), Millis(20), Millis(1000)) {
            assert_eq!(channel, ChannelId::ReliableInput);
            assert_eq!(seq, Sequence(9));
            return;
        }
    }
    panic!("tinha de desistir");
}

#[test]
fn control_is_swept_before_input() {
    // Sequências distintas para que a ordem do resultado seja inequívoca.
    const INPUT: u32 = 20;
    const CONTROL: u32 = 10;

    let mut channels = ReliableChannels::new();
    channels.on_sent(
        ChannelId::ReliableInput,
        at(0),
        Sequence(INPUT),
        &frame(INPUT),
    );
    channels.on_sent(
        ChannelId::Control,
        at(0),
        Sequence(CONTROL),
        &frame(CONTROL),
    );

    match channels.on_tick(at(50), Millis(20), Millis(1000)) {
        Due::Retransmit(frames) => {
            let order: Vec<u32> = frames.iter().map(|f| f.seq.get()).collect();
            assert_eq!(
                order,
                vec![CONTROL, INPUT],
                "controle primeiro, na ordem de importância"
            );
        }
        other => panic!("deveria retransmitir os dois, deu {other:?}"),
    }
}

#[test]
fn resetting_clears_every_channel() {
    let mut channels = ReliableChannels::new();
    for channel in ReliableChannels::COVERED {
        channels.on_sent(channel, at(0), Sequence(1), &frame(1));
        channels.accept(channel, frame(1));
    }
    channels.reset();
    for channel in ReliableChannels::COVERED {
        assert_eq!(channels.pending(channel), 0);
        assert!(channels.ack_for(channel).is_none());
    }
}

#[test]
fn resetting_the_receivers_keeps_what_was_sent() {
    let mut channels = ReliableChannels::new();
    for channel in ReliableChannels::COVERED {
        channels.on_sent(channel, at(0), Sequence(1), &frame(1));
        channels.accept(channel, frame(1));
    }
    channels.reset_receivers();
    for channel in ReliableChannels::COVERED {
        assert_eq!(channels.pending(channel), 1, "{channel}: o emissor fica");
        assert!(channels.ack_for(channel).is_none(), "{channel}");
        assert!(
            matches!(channels.accept(channel, frame(1)), Delivery::Ready(_)),
            "{channel}: o que o par manda de novo é novo"
        );
    }
}
