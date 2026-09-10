//! O envelope: canal, sequência e confirmação.
//!
//! Um quadro é o que a camada de criptografia cifra e o portador entrega
//! (`docs/03-protocolo.md` §1). O canal vem primeiro no fio, porque é ele que diz como
//! interpretar o resto.

use serde::{Deserialize, Serialize};

use crate::channel::ChannelId;
use crate::message::Message;

/// Número de sequência de um canal.
///
/// A comparação **não** é a de inteiro. Depois de 2³² mensagens o contador dá a volta, e
/// `4_294_967_295 < 0` é falso para `u32` mas verdadeiro para sequência. Usar comparação
/// ingênua faria o canal do ponteiro travar para sempre no momento da volta — um bug que
/// só aparece depois de horas de uso, e por isso mesmo é caro de descobrir em produção.
///
/// A regra é a aritmética de número de série da RFC 1982: `a` é mais nova que `b` quando a
/// distância de `b` até `a`, dando a volta, cai na primeira metade do espaço.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Default, Hash, Serialize, Deserialize, PartialOrd, Ord,
)]
#[serde(transparent)]
pub struct Sequence(pub u32);

impl Sequence {
    /// A primeira sequência de um canal.
    pub const ZERO: Self = Self(0);

    /// Metade do espaço de sequência: a fronteira entre "mais nova" e "mais velha".
    const HALF_SPACE: u32 = 1 << 31;

    /// A sequência seguinte, dando a volta em vez de estourar.
    #[must_use]
    pub const fn next(self) -> Self {
        Self(self.0.wrapping_add(1))
    }

    /// Se `self` é mais nova que `other`, com a volta tratada corretamente.
    ///
    /// Metade do espaço à frente conta como "mais nova"; a outra metade, como "mais velha".
    /// É a única definição que funciona sem guardar histórico.
    #[must_use]
    pub const fn is_newer_than(self, other: Self) -> bool {
        let ahead = self.0.wrapping_sub(other.0);
        ahead != 0 && ahead < Self::HALF_SPACE
    }

    /// Quantas mensagens `self` está à frente de `other`, dando a volta.
    #[must_use]
    pub const fn distance_from(self, other: Self) -> u32 {
        self.0.wrapping_sub(other.0)
    }

    /// O número cru.
    #[must_use]
    pub const fn get(self) -> u32 {
        self.0
    }
}

/// Confirmação de recebimento, para os canais confiáveis sobre UDP.
///
/// Sobre RFCOMM e TCP o portador já garante ordem e entrega, e este campo fica ausente —
/// custando 1 byte de `None` em vez dos 8 de um `Ack` que ninguém usaria
/// (`docs/03-protocolo.md` §4.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Ack {
    /// Maior sequência recebida de forma contígua.
    pub cumulative: Sequence,
    /// As 32 sequências anteriores a [`Self::cumulative`], em bitmap.
    ///
    /// Bit 0 é `cumulative - 1`, bit 31 é `cumulative - 32`. Permite ao emissor saber
    /// exatamente o que falta sem esperar um tempo de retransmissão por mensagem.
    pub bits: u32,
}

impl Ack {
    /// Confirmação sem nenhuma sequência anterior marcada.
    #[must_use]
    pub const fn new(cumulative: Sequence) -> Self {
        Self {
            cumulative,
            bits: 0,
        }
    }

    /// Se esta confirmação cobre a sequência dada.
    #[must_use]
    pub const fn covers(self, seq: Sequence) -> bool {
        if seq.0 == self.cumulative.0 {
            return true;
        }
        if seq.is_newer_than(self.cumulative) {
            return false;
        }
        let behind = self.cumulative.distance_from(seq);
        if behind == 0 || behind > 32 {
            return false;
        }
        self.bits & (1u32 << (behind - 1)) != 0
    }

    /// Marca a sequência dada como recebida, se ela couber na janela.
    #[must_use]
    pub const fn with(mut self, seq: Sequence) -> Self {
        let behind = self.cumulative.distance_from(seq);
        if behind >= 1 && behind <= 32 {
            self.bits |= 1u32 << (behind - 1);
        }
        self
    }
}

/// Um quadro do protocolo, pronto para ser cifrado e enviado.
///
/// A ordem dos campos **é o formato de fio** e não pode ser trocada sem incremento de
/// `version::CURRENT` — `postcard` é posicional (`docs/03-protocolo.md` §9). A mensagem vem
/// primeiro justamente para que o discriminante de canal seja o primeiro byte, como a
/// especificação exige.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Frame {
    /// A mensagem. O canal é determinado pela variante.
    pub message: Message,
    /// Sequência desta mensagem, dentro do seu canal.
    pub seq: Sequence,
    /// Confirmação carregada de volta, quando o portador precisa dela.
    pub ack: Option<Ack>,
}

impl Frame {
    /// Monta um quadro sem confirmação carregada.
    #[must_use]
    pub const fn new(message: Message, seq: Sequence) -> Self {
        Self {
            message,
            seq,
            ack: None,
        }
    }

    /// O mesmo quadro, carregando uma confirmação.
    #[must_use]
    pub fn with_ack(mut self, ack: Ack) -> Self {
        self.ack = Some(ack);
        self
    }

    /// O canal deste quadro.
    #[must_use]
    pub const fn channel(&self) -> ChannelId {
        self.message.channel()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn next_wraps_instead_of_overflowing() {
        assert_eq!(Sequence(u32::MAX).next(), Sequence::ZERO);
    }

    #[test]
    fn newer_comparison_survives_the_wraparound() {
        // O caso que a comparação ingênua erra: 0 vem depois de u32::MAX.
        assert!(Sequence(0).is_newer_than(Sequence(u32::MAX)));
        assert!(!Sequence(u32::MAX).is_newer_than(Sequence(0)));

        assert!(Sequence(5).is_newer_than(Sequence(4)));
        assert!(!Sequence(4).is_newer_than(Sequence(5)));
        assert!(
            !Sequence(4).is_newer_than(Sequence(4)),
            "igual não é mais nova"
        );
    }

    #[test]
    fn newer_comparison_is_consistent_across_the_whole_wrap_boundary() {
        let base = Sequence(u32::MAX - 3);
        let mut previous = base;
        for _ in 0..10 {
            let current = previous.next();
            assert!(
                current.is_newer_than(previous),
                "{current:?} deveria ser mais nova"
            );
            assert!(!previous.is_newer_than(current));
            previous = current;
        }
    }

    #[test]
    fn half_the_space_ahead_is_newer_and_the_other_half_is_older() {
        let zero = Sequence::ZERO;
        let just_under_half = Sequence(u32::MAX / 2);
        let just_over_half = Sequence(u32::MAX / 2 + 2);
        assert!(just_under_half.is_newer_than(zero));
        assert!(!just_over_half.is_newer_than(zero));
    }

    #[test]
    fn distance_wraps() {
        assert_eq!(Sequence(2).distance_from(Sequence(u32::MAX)), 3);
        assert_eq!(Sequence(7).distance_from(Sequence(4)), 3);
    }

    #[test]
    fn ack_covers_its_own_cumulative_sequence() {
        let ack = Ack::new(Sequence(100));
        assert!(ack.covers(Sequence(100)));
    }

    #[test]
    fn ack_does_not_cover_anything_newer() {
        let ack = Ack::new(Sequence(100));
        assert!(!ack.covers(Sequence(101)));
        assert!(!ack.covers(Sequence(500)));
    }

    #[test]
    fn ack_bitmap_marks_and_reports_earlier_sequences() {
        let ack = Ack::new(Sequence(100))
            .with(Sequence(99))
            .with(Sequence(68));
        assert!(ack.covers(Sequence(99)), "1 atrás");
        assert!(ack.covers(Sequence(68)), "32 atrás, o limite da janela");
        assert!(!ack.covers(Sequence(98)), "não marcada");
        assert!(!ack.covers(Sequence(67)), "fora da janela de 32");
    }

    #[test]
    fn ack_window_ignores_sequences_too_far_behind() {
        let ack = Ack::new(Sequence(100)).with(Sequence(10));
        assert_eq!(ack.bits, 0, "fora da janela não deve marcar bit nenhum");
    }

    #[test]
    fn ack_bitmap_works_across_the_wraparound() {
        let ack = Ack::new(Sequence(2))
            .with(Sequence(u32::MAX))
            .with(Sequence(0));
        assert!(
            ack.covers(Sequence(u32::MAX)),
            "3 atrás, atravessando a volta"
        );
        assert!(ack.covers(Sequence(0)), "2 atrás");
        assert!(!ack.covers(Sequence(1)), "1 atrás, não marcada");
    }
}
