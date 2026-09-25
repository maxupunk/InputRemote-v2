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
    /// Os canais com confiabilidade de aplicação — a única lista deles; [`Self::pair`] é o mapa.
    pub const COVERED: [ChannelId; 4] = [
        ChannelId::Control,
        ChannelId::ReliableInput,
        ChannelId::Feedback,
        ChannelId::ClipboardText,
    ];

    /// A ordem de varredura das retransmissões: controle antes de entrada — se o controle desistiu,
    /// não faz sentido reenviar teclas por um enlace que vai cair.
    pub const RETRANSMIT_ORDER: [ChannelId; 4] = Self::COVERED;

    /// A ordem de urgência das confirmações a carregar: entrada antes de controle, porque é a janela
    /// da entrada que enche durante digitação contínua e derrubaria a sessão no meio de uma frase.
    pub const ACK_ORDER: [ChannelId; 4] = [
        ChannelId::ReliableInput,
        ChannelId::Control,
        ChannelId::Feedback,
        ChannelId::ClipboardText,
    ];

    /// Todos os pares vazios.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Se este canal tem confiabilidade de aplicação.
    #[must_use]
    pub fn covers(channel: ChannelId) -> bool {
        Self::COVERED.contains(&channel)
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
    /// Varre os canais em [`Self::RETRANSMIT_ORDER`]. Se algum desistiu, isso é reportado antes
    /// de qualquer retransmissão.
    pub fn on_tick(&mut self, now: Timestamp, floor: Millis, ceiling: Millis) -> Due {
        let mut resend = Vec::new();
        for channel in Self::RETRANSMIT_ORDER {
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
        for channel in Self::COVERED {
            if let Some(pair) = self.pair_mut(channel) {
                pair.receiver = Receiver::new();
            }
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
