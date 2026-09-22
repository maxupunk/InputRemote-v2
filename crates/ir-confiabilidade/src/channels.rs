//! Um par emissor/receptor por canal confiável.
//!
//! Só os quatro canais confiáveis que podem viajar por datagrama têm par aqui. O canal do
//! ponteiro não tem, porque nele o mais recente vence e retransmitir amostra antiga seria
//! trabalho para entregar informação já obsoleta. O canal de dados não tem, porque ele só
//! existe sobre TCP, que já garante entrega.
//!
//! Um par por canal, e não um só para tudo: a retransmissão de um bloco de clipboard não pode
//! atrasar um `KeyUp`, e uma janela compartilhada faria exatamente isso.

use ir_proto::channel::ChannelId;
use ir_proto::frame::{Ack, Frame, Sequence};

use crate::time::{Millis, Timestamp};

use crate::receiver::{Delivery, Receiver};
use crate::sender::{SendOutcome, Sender, TimeoutOutcome};

/// O emissor e o receptor de um canal.
#[derive(Debug, Clone, Default)]
struct Pair {
    sender: Sender,
    receiver: Receiver,
}

/// Os pares de todos os canais confiáveis sobre datagrama.
#[derive(Debug, Clone, Default)]
pub struct ReliableChannels {
    control: Pair,
    input: Pair,
    feedback: Pair,
    clipboard: Pair,
}

/// O que a passagem do tempo pede, e em qual canal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Due {
    /// Nada vencido em nenhum canal.
    Idle,
    /// Reenvie estes quadros.
    Retransmit(Vec<Frame>),
    /// Um canal passou do prazo sem confirmação. Derrube o enlace.
    GiveUp {
        /// Em qual canal.
        channel: ChannelId,
        /// Qual sequência nunca foi confirmada.
        seq: Sequence,
    },
}

impl ReliableChannels {
    /// Todos os pares vazios.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Se este canal tem confiabilidade de aplicação.
    #[must_use]
    pub const fn covers(channel: ChannelId) -> bool {
        matches!(
            channel,
            ChannelId::Control
                | ChannelId::ReliableInput
                | ChannelId::Feedback
                | ChannelId::ClipboardText
        )
    }

    /// Registra um envio.
    ///
    /// Devolve `Accepted` para canais que não são cobertos: eles não precisam de janela, e
    /// tratá-los como aceitos mantém o chamador simples.
    pub fn on_sent(
        &mut self,
        channel: ChannelId,
        now: Timestamp,
        seq: Sequence,
        frame: &Frame,
    ) -> SendOutcome {
        match self.pair_mut(channel) {
            Some(pair) => pair.sender.on_sent(now, seq, frame.clone()),
            None => SendOutcome::Accepted,
        }
    }

    /// Aplica uma confirmação vinda do par.
    pub fn on_ack(&mut self, channel: ChannelId, now: Timestamp, ack: Ack) {
        if let Some(pair) = self.pair_mut(channel) {
            pair.sender.on_ack(now, ack);
        }
    }

    /// Registra um quadro recebido e diz o que fazer com ele.
    ///
    /// Canais não cobertos entregam direto: o do ponteiro faz o próprio descarte por
    /// sequência, e o de dados vem por TCP, que já entrega em ordem.
    pub fn accept(&mut self, channel: ChannelId, frame: Frame) -> Delivery {
        let seq = frame.seq;
        match self.pair_mut(channel) {
            Some(pair) => pair.receiver.accept(seq, frame),
            None => Delivery::Ready(vec![frame]),
        }
    }

    /// Quantos quadros estão esperando reordenação neste canal.
    #[must_use]
    pub fn buffered(&self, channel: ChannelId) -> usize {
        self.pair(channel)
            .map_or(0, |pair| pair.receiver.buffered())
    }

    /// A confirmação a carregar num quadro deste canal.
    #[must_use]
    pub fn ack_for(&self, channel: ChannelId) -> Option<Ack> {
        self.pair(channel)
            .and_then(|pair| pair.receiver.ack_to_send())
    }

    /// Quantas mensagens estão sem confirmação neste canal.
    #[must_use]
    pub fn pending(&self, channel: ChannelId) -> usize {
        self.pair(channel).map_or(0, |pair| pair.sender.pending())
    }

    /// Se a mensagem de sequência `next` pode sair sem ficar fora do alcance da confirmação.
    /// Canais não cobertos sempre podem.
    #[must_use]
    pub fn within_ack_reach(&self, channel: ChannelId, next: Sequence) -> bool {
        self.pair(channel)
            .is_none_or(|pair| pair.sender.within_ack_reach(next))
    }

    /// O que a passagem do tempo pede.
    ///
    /// Varre os canais na ordem de importância: controle primeiro, entrada em seguida. Se
    /// algum desistiu, isso é reportado antes de qualquer retransmissão — não faz sentido
    /// reenviar por um enlace que vai cair.
    pub fn on_tick(&mut self, now: Timestamp, floor: Millis, ceiling: Millis) -> Due {
        const ORDER: [ChannelId; 4] = [
            ChannelId::Control,
            ChannelId::ReliableInput,
            ChannelId::Feedback,
            ChannelId::ClipboardText,
        ];

        let mut resend = Vec::new();
        for channel in ORDER {
            let Some(pair) = self.pair_mut(channel) else {
                continue;
            };
            match pair.sender.on_tick(now, floor, ceiling) {
                TimeoutOutcome::Idle => {}
                TimeoutOutcome::Retransmit(frames) => resend.extend(frames),
                TimeoutOutcome::GiveUp { seq } => return Due::GiveUp { channel, seq },
            }
        }

        if resend.is_empty() {
            Due::Idle
        } else {
            Due::Retransmit(resend)
        }
    }

    /// Esvazia só o lado receptor.
    ///
    /// O par começou outra encarnação, e o que ele mandou antes não conta mais. O lado emissor
    /// fica: ele carrega o que **esta** ponta já mandou na encarnação corrente.
    pub fn reset_receivers(&mut self) {
        for pair in [
            &mut self.control,
            &mut self.input,
            &mut self.feedback,
            &mut self.clipboard,
        ] {
            pair.receiver = Receiver::new();
        }
    }

    /// Esvazia tudo. Chamado a cada handshake novo.
    pub fn reset(&mut self) {
        *self = Self::new();
    }

    const fn pair(&self, channel: ChannelId) -> Option<&Pair> {
        match channel {
            ChannelId::Control => Some(&self.control),
            ChannelId::ReliableInput => Some(&self.input),
            ChannelId::Feedback => Some(&self.feedback),
            ChannelId::ClipboardText => Some(&self.clipboard),
            ChannelId::Pointer | ChannelId::Bulk => None,
        }
    }

    const fn pair_mut(&mut self, channel: ChannelId) -> Option<&mut Pair> {
        match channel {
            ChannelId::Control => Some(&mut self.control),
            ChannelId::ReliableInput => Some(&mut self.input),
            ChannelId::Feedback => Some(&mut self.feedback),
            ChannelId::ClipboardText => Some(&mut self.clipboard),
            ChannelId::Pointer | ChannelId::Bulk => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ir_proto::input::{HidUsage, Modifiers};
    use ir_proto::message::{InputMessage, Message};

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
    fn the_pointer_and_bulk_channels_are_not_covered() {
        assert!(!ReliableChannels::covers(ChannelId::Pointer));
        assert!(!ReliableChannels::covers(ChannelId::Bulk));
        for channel in [
            ChannelId::Control,
            ChannelId::ReliableInput,
            ChannelId::Feedback,
            ChannelId::ClipboardText,
        ] {
            assert!(
                ReliableChannels::covers(channel),
                "{channel} deveria ser coberto"
            );
        }
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
            if let Due::GiveUp { channel, seq } =
                channels.on_tick(at(now), Millis(20), Millis(1000))
            {
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
        for channel in [ChannelId::Control, ChannelId::ReliableInput] {
            channels.on_sent(channel, at(0), Sequence(1), &frame(1));
            channels.accept(channel, frame(1));
        }
        channels.reset();
        for channel in [ChannelId::Control, ChannelId::ReliableInput] {
            assert_eq!(channels.pending(channel), 0);
            assert!(channels.ack_for(channel).is_none());
        }
    }
}
