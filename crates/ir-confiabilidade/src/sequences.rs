//! Um contador de sequência por canal.
//!
//! Cada canal numera as próprias mensagens. Um contador único para todos os canais faria o
//! canal do ponteiro — que descarta o que chega atrasado — descartar mensagens de teclado
//! que passaram na frente, o que é exatamente o defeito que não se pode ter.
//!
//! Sem indexação de fatia: um campo por canal, escolhido por `match`. É mais verboso e não
//! tem caminho de pânico (`docs/09-padroes-de-codigo.md` §5).

use ir_proto::channel::ChannelId;
use ir_proto::frame::Sequence;

/// Os seis contadores.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Sequences {
    control: Sequence,
    input: Sequence,
    pointer: Sequence,
    feedback: Sequence,
    clipboard: Sequence,
    bulk: Sequence,
}

impl Sequences {
    /// Todos em zero.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            control: Sequence::ZERO,
            input: Sequence::ZERO,
            pointer: Sequence::ZERO,
            feedback: Sequence::ZERO,
            clipboard: Sequence::ZERO,
            bulk: Sequence::ZERO,
        }
    }

    /// A próxima sequência deste canal, avançando o contador.
    pub const fn next(&mut self, channel: ChannelId) -> Sequence {
        let slot = self.slot_mut(channel);
        let current = *slot;
        *slot = current.next();
        current
    }

    /// A sequência que este canal vai usar em seguida, sem avançar.
    #[must_use]
    pub const fn peek(&self, channel: ChannelId) -> Sequence {
        match channel {
            ChannelId::Control => self.control,
            ChannelId::ReliableInput => self.input,
            ChannelId::Pointer => self.pointer,
            ChannelId::Feedback => self.feedback,
            ChannelId::ClipboardText => self.clipboard,
            ChannelId::Bulk => self.bulk,
        }
    }

    /// Zera tudo.
    ///
    /// Chamado a cada handshake novo: as duas pontas recomeçam do zero, e uma sequência
    /// herdada da sessão anterior faria o par descartar as primeiras mensagens da nova.
    pub const fn reset(&mut self) {
        *self = Self::new();
    }

    const fn slot_mut(&mut self, channel: ChannelId) -> &mut Sequence {
        match channel {
            ChannelId::Control => &mut self.control,
            ChannelId::ReliableInput => &mut self.input,
            ChannelId::Pointer => &mut self.pointer,
            ChannelId::Feedback => &mut self.feedback,
            ChannelId::ClipboardText => &mut self.clipboard,
            ChannelId::Bulk => &mut self.bulk,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_channel_starts_at_zero() {
        let seqs = Sequences::new();
        for channel in ChannelId::ALL {
            assert_eq!(seqs.peek(channel), Sequence::ZERO, "{channel}");
        }
    }

    #[test]
    fn next_returns_the_current_value_and_advances() {
        let mut seqs = Sequences::new();
        assert_eq!(seqs.next(ChannelId::Control), Sequence(0));
        assert_eq!(seqs.next(ChannelId::Control), Sequence(1));
        assert_eq!(seqs.peek(ChannelId::Control), Sequence(2));
    }

    #[test]
    fn the_channels_are_independent() {
        let mut seqs = Sequences::new();
        for _ in 0..5 {
            seqs.next(ChannelId::Pointer);
        }
        assert_eq!(seqs.peek(ChannelId::Pointer), Sequence(5));
        assert_eq!(
            seqs.peek(ChannelId::ReliableInput),
            Sequence::ZERO,
            "o teclado não pode herdar a contagem do ponteiro"
        );
    }

    #[test]
    fn every_channel_advances_only_itself() {
        for advanced in ChannelId::ALL {
            let mut seqs = Sequences::new();
            seqs.next(advanced);
            for other in ChannelId::ALL {
                let expected = if other == advanced {
                    Sequence(1)
                } else {
                    Sequence::ZERO
                };
                assert_eq!(seqs.peek(other), expected, "{advanced} mexeu em {other}");
            }
        }
    }

    #[test]
    fn reset_clears_every_channel() {
        let mut seqs = Sequences::new();
        for channel in ChannelId::ALL {
            seqs.next(channel);
        }
        seqs.reset();
        assert_eq!(seqs, Sequences::new());
    }

    #[test]
    fn the_counter_wraps_instead_of_overflowing() {
        let mut seqs = Sequences::new();
        for _ in 0..3 {
            seqs.next(ChannelId::Control);
        }
        // Salto até o fim da faixa, para exercer a volta sem 4 bilhões de iterações.
        seqs.control = Sequence(u32::MAX);
        assert_eq!(seqs.next(ChannelId::Control), Sequence(u32::MAX));
        assert_eq!(seqs.peek(ChannelId::Control), Sequence::ZERO);
    }
}
