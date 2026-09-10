//! Tempo, injetado.
//!
//! Este crate **nunca lê o relógio**. Todo instante entra por parâmetro, em
//! [`Input::Tick`](crate::Input::Tick) ou junto de um evento. É a regra de
//! `docs/09-padroes-de-codigo.md` §3, e o motivo é prático: um cenário de reconexão de trinta
//! segundos precisa rodar em microssegundos no teste, e de forma determinística.
//!
//! Não se usa `std::time::Instant` porque ele não pode ser construído com um valor escolhido
//! — o que tornaria impossível escrever "às 12h34 o par sumiu" num teste. `Timestamp` é um
//! `u64` opaco, e só a periferia sabe de onde ele veio.

/// Um instante monotônico, em microssegundos desde uma origem arbitrária.
///
/// A origem não tem significado: só diferenças entre dois `Timestamp` importam. Isso é
/// deliberado — nunca é preciso relógio comum entre as duas máquinas
/// (`docs/03-protocolo.md` §6, `Ping`/`Pong`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Timestamp(u64);

/// Uma duração, em milissegundos.
///
/// Milissegundos e não microssegundos porque nenhum prazo deste crate é mais fino que isso,
/// e porque um número redondo é mais fácil de conferir num teste.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Millis(pub u32);

impl Timestamp {
    /// A origem da escala.
    pub const ZERO: Self = Self(0);

    /// Um instante, em microssegundos desde a origem.
    #[must_use]
    pub const fn from_micros(micros: u64) -> Self {
        Self(micros)
    }

    /// Um instante, em milissegundos desde a origem.
    #[must_use]
    pub const fn from_millis(millis: u64) -> Self {
        Self(millis.saturating_mul(1000))
    }

    /// O valor cru, em microssegundos.
    #[must_use]
    pub const fn micros(self) -> u64 {
        self.0
    }

    /// Este instante mais uma duração, saturando.
    ///
    /// Não é `const` porque `u64::from` ainda não é utilizável em contexto constante, e
    /// preferir a conversão sem perda a um `as` é o que a política de lints exige.
    #[must_use]
    pub fn plus(self, duration: Millis) -> Self {
        Self(
            self.0
                .saturating_add(u64::from(duration.0).saturating_mul(1000)),
        )
    }

    /// Quanto tempo passou de `earlier` até `self`.
    ///
    /// Zero quando `earlier` é posterior. Um relógio monotônico não anda para trás, mas um
    /// carimbo pode chegar fora de ordem por uma fila, e uma duração negativa não existe
    /// neste tipo — o que elimina uma classe inteira de erro de sinal.
    #[must_use]
    pub fn since(self, earlier: Self) -> Millis {
        let millis = self.0.saturating_sub(earlier.0) / 1000;
        // Satura em vez de truncar: 49 dias de diferença é absurdo, mas dar a volta seria
        // pior — um prazo vencido pareceria não vencido.
        Millis(u32::try_from(millis).unwrap_or(u32::MAX))
    }

    /// Se já passou pelo menos `duration` desde `earlier`.
    #[must_use]
    pub fn elapsed_at_least(self, earlier: Self, duration: Millis) -> bool {
        self.since(earlier).0 >= duration.0
    }
}

impl Millis {
    /// Nenhum tempo.
    pub const ZERO: Self = Self(0);

    /// Em milissegundos.
    #[must_use]
    pub const fn get(self) -> u32 {
        self.0
    }

    /// A maior das duas.
    #[must_use]
    pub const fn max(self, other: Self) -> Self {
        if self.0 > other.0 { self } else { other }
    }

    /// A menor das duas.
    #[must_use]
    pub const fn min(self, other: Self) -> Self {
        if self.0 < other.0 { self } else { other }
    }

    /// Esta duração multiplicada, saturando.
    #[must_use]
    pub const fn times(self, factor: u32) -> Self {
        Self(self.0.saturating_mul(factor))
    }
}

impl core::fmt::Display for Millis {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{} ms", self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn since_measures_forward_time() {
        let start = Timestamp::from_millis(1000);
        let later = Timestamp::from_millis(1250);
        assert_eq!(later.since(start), Millis(250));
    }

    #[test]
    fn since_never_goes_negative() {
        let start = Timestamp::from_millis(1000);
        let earlier = Timestamp::from_millis(500);
        assert_eq!(
            earlier.since(start),
            Millis::ZERO,
            "duração negativa não existe"
        );
    }

    #[test]
    fn plus_advances_by_the_duration() {
        let start = Timestamp::from_millis(1000);
        assert_eq!(start.plus(Millis(250)), Timestamp::from_millis(1250));
    }

    #[test]
    fn plus_saturates_at_the_end_of_the_scale() {
        let late = Timestamp::from_micros(u64::MAX - 10);
        assert_eq!(
            late.plus(Millis(u32::MAX)),
            Timestamp::from_micros(u64::MAX)
        );
    }

    #[test]
    fn elapsed_is_inclusive_at_the_boundary() {
        let start = Timestamp::from_millis(0);
        let exact = Timestamp::from_millis(250);
        assert!(
            exact.elapsed_at_least(start, Millis(250)),
            "exatamente no prazo conta"
        );
        assert!(!exact.elapsed_at_least(start, Millis(251)));
    }

    #[test]
    fn sub_millisecond_differences_round_down() {
        let start = Timestamp::from_micros(0);
        let almost = Timestamp::from_micros(999);
        assert_eq!(almost.since(start), Millis::ZERO);
        assert_eq!(Timestamp::from_micros(1000).since(start), Millis(1));
    }

    #[test]
    fn timestamps_order_like_time() {
        assert!(Timestamp::from_millis(1) < Timestamp::from_millis(2));
        assert_eq!(Timestamp::ZERO, Timestamp::from_micros(0));
    }

    #[test]
    fn millis_helpers_behave() {
        assert_eq!(Millis(10).max(Millis(20)), Millis(20));
        assert_eq!(Millis(10).min(Millis(20)), Millis(10));
        assert_eq!(Millis(10).times(3), Millis(30));
        assert_eq!(Millis(u32::MAX).times(2), Millis(u32::MAX), "satura");
        assert_eq!(Millis(250).to_string(), "250 ms");
    }
}
