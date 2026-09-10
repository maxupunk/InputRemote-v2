//! O lado emissor: janela, retransmissão e desistência.

use std::collections::VecDeque;

use ir_proto::frame::{Ack, Frame, Sequence};

use crate::time::{Millis, Timestamp};

/// Quantas mensagens podem estar sem confirmação ao mesmo tempo.
///
/// Origem: `docs/03-protocolo.md` §4.1. O bitmap de confirmação cobre 32 sequências, então
/// uma janela de 64 é o dobro do que o receptor consegue descrever de uma vez — o suficiente
/// para uma rajada de digitação e pequeno o bastante para não esconder um enlace ruim.
pub const WINDOW: usize = 64;

/// Uma mensagem enviada e ainda não confirmada.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Pending {
    seq: Sequence,
    frame: Frame,
    sent_at: Timestamp,
    tries: u8,
}

/// O que aconteceu ao tentar registrar um envio.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SendOutcome {
    /// Registrado; pode mandar.
    Accepted,
    /// A janela está cheia.
    ///
    /// O chamador **deve** derrubar o enlace. Descartar seria perder um evento de teclado em
    /// silêncio, e é justamente isso que não se pode fazer — a política de saturação de
    /// `ChannelId::saturation` para canal confiável é `FailLink`.
    WindowFull,
}

/// O que a passagem do tempo pede.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TimeoutOutcome {
    /// Nada vencido.
    Idle,
    /// Reenvie estes quadros.
    Retransmit(Vec<Frame>),
    /// Uma mensagem esgotou as tentativas. Derrube o enlace.
    GiveUp {
        /// A sequência que nunca foi confirmada, para o diagnóstico.
        seq: Sequence,
    },
}

/// O lado emissor de um canal confiável.
#[derive(Debug, Clone, Default)]
pub struct Sender {
    unacked: VecDeque<Pending>,
    /// Média móvel do tempo de ida e volta, quando já houve amostra.
    srtt: Option<Millis>,
}

impl Sender {
    /// Um emissor sem nada pendente.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            unacked: VecDeque::new(),
            srtt: None,
        }
    }

    /// Quantas mensagens estão sem confirmação.
    #[must_use]
    pub fn pending(&self) -> usize {
        self.unacked.len()
    }

    /// Se não há nada esperando confirmação.
    #[must_use]
    pub fn is_idle(&self) -> bool {
        self.unacked.is_empty()
    }

    /// Registra um envio.
    ///
    /// Guarda o quadro para poder reenviá-lo. Quem chama envia de fato; este módulo não faz
    /// E/S nenhuma.
    pub fn on_sent(&mut self, now: Timestamp, seq: Sequence, frame: Frame) -> SendOutcome {
        if self.unacked.len() >= WINDOW {
            return SendOutcome::WindowFull;
        }
        self.unacked.push_back(Pending {
            seq,
            frame,
            sent_at: now,
            tries: 1,
        });
        SendOutcome::Accepted
    }

    /// Aplica uma confirmação vinda do par.
    ///
    /// Atualiza a média de ida e volta com a mensagem mais antiga confirmada — a mais antiga,
    /// e não a mais nova, porque ela é a que esperou mais e portanto a que revela o pior caso
    /// que o enlace está entregando.
    pub fn on_ack(&mut self, now: Timestamp, ack: Ack) {
        let mut oldest_sample = None;
        self.unacked.retain(|pending| {
            if ack.covers(pending.seq) {
                if pending.tries == 1 {
                    // Só amostra quem não foi retransmitido: o tempo de uma retransmissão
                    // mede o prazo de retransmissão, não o enlace.
                    let sample = now.since(pending.sent_at);
                    oldest_sample = Some(oldest_sample.map_or(sample, |s: Millis| s.max(sample)));
                }
                false
            } else {
                true
            }
        });
        if let Some(sample) = oldest_sample {
            self.srtt = Some(match self.srtt {
                // Suavização de 1/8, como em TCP: reage a mudança sem oscilar com uma amostra.
                Some(current) => {
                    Millis(current.get().saturating_mul(7).saturating_add(sample.get()) / 8)
                }
                None => sample,
            });
        }
    }

    /// O prazo de retransmissão corrente.
    ///
    /// `max(piso, 2 × srtt)`, limitado para não passar do prazo de queda — retransmitir
    /// depois de o enlace já ter sido declarado morto não serve para nada.
    #[must_use]
    pub fn retransmit_after(&self, floor: Millis, ceiling: Millis) -> Millis {
        let doubled = self.srtt.map_or(floor, |srtt| srtt.times(2));
        doubled.max(floor).min(ceiling)
    }

    /// O que a passagem do tempo pede.
    pub fn on_tick(
        &mut self,
        now: Timestamp,
        floor: Millis,
        ceiling: Millis,
        max_tries: u8,
    ) -> TimeoutOutcome {
        let rto = self.retransmit_after(floor, ceiling);

        // Desistir vem antes de retransmitir: se alguma mensagem já esgotou as tentativas,
        // não faz sentido reenviar as outras por um enlace que vai cair.
        if let Some(dead) = self
            .unacked
            .iter()
            .find(|p| p.tries >= max_tries && now.elapsed_at_least(p.sent_at, rto))
        {
            return TimeoutOutcome::GiveUp { seq: dead.seq };
        }

        let mut resend = Vec::new();
        for pending in &mut self.unacked {
            if now.elapsed_at_least(pending.sent_at, rto) {
                pending.tries = pending.tries.saturating_add(1);
                pending.sent_at = now;
                resend.push(pending.frame.clone());
            }
        }

        if resend.is_empty() {
            TimeoutOutcome::Idle
        } else {
            TimeoutOutcome::Retransmit(resend)
        }
    }

    /// Esvazia tudo. Chamado a cada handshake novo.
    pub fn reset(&mut self) {
        self.unacked.clear();
        self.srtt = None;
    }
}
