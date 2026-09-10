//! A janela deslizante contra repetição, de 2 048 bits.
//!
//! O contador de cada quadro sobre UDP é explícito, porque a ordem não é garantida
//! ([03, §3.1](../../../docs/03-protocolo.md)). Um contador já visto, ou antigo demais para
//! caber na janela, é descartado sem processar — é o que impede reenviar um `KeyDown` gravado do
//! rádio ([04, §2](../../../docs/04-seguranca.md)).
//!
//! O algoritmo é o de janela deslizante do IPsec (RFC 6479): um `highest` visto e um mapa
//! de bits em que o bit `k` representa o contador `highest - k`.

/// Largura da janela, em bits.
const WINDOW: u64 = 2048;
/// Palavras de 64 bits que compõem a janela.
const WORDS: usize = (WINDOW / 64) as usize;

/// A janela de repetição de um sentido de transporte.
#[derive(Debug, Clone)]
pub struct ReplayWindow {
    /// O maior contador já aceito. Zero significa "nada visto ainda".
    highest: u64,
    /// Bit `k` = contador `highest - k`. Bit 0 = o próprio `highest`.
    bits: [u64; WORDS],
}

impl Default for ReplayWindow {
    fn default() -> Self {
        Self::new()
    }
}

impl ReplayWindow {
    /// Uma janela nova, sem nada visto.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            highest: 0,
            bits: [0; WORDS],
        }
    }

    /// Aceita um contador, marcando-o como visto. `false` significa repetição ou antigo demais.
    ///
    /// O contador zero é sempre recusado: o transporte só emite a partir de 1, então zero só
    /// pode ser lixo ou uma tentativa de reuso do nonce inicial.
    pub fn accept(&mut self, counter: u64) -> bool {
        if counter == 0 {
            return false;
        }
        if counter > self.highest {
            self.advance_to(counter);
            return true;
        }
        let behind = self.highest - counter;
        if behind >= WINDOW {
            return false; // antigo demais para a janela
        }
        if self.get(behind) {
            return false; // já visto
        }
        self.set(behind);
        true
    }

    /// Se este contador seria aceito, sem marcá-lo como visto.
    ///
    /// Serve para conferir a janela **antes** de decifrar, sem consumir o contador caso a
    /// decifragem falhe — um quadro forjado não pode queimar o número do legítimo.
    #[must_use]
    pub fn would_accept(&self, counter: u64) -> bool {
        if counter == 0 {
            return false;
        }
        if counter > self.highest {
            return true;
        }
        let behind = self.highest - counter;
        behind < WINDOW && !self.get(behind)
    }

    /// Avança a janela para um novo `highest`, deslocando os bits.
    fn advance_to(&mut self, counter: u64) {
        let shift = counter - self.highest;
        if shift >= WINDOW {
            self.bits = [0; WORDS];
        } else {
            self.shift_up(usize::try_from(shift).unwrap_or(WORDS * 64));
        }
        self.highest = counter;
        self.set(0);
    }

    /// Desloca os bits para offsets maiores (contadores mais antigos), descartando os que caem.
    fn shift_up(&mut self, shift: usize) {
        let word_shift = shift / 64;
        let bit_shift = shift % 64;
        let mut out = [0u64; WORDS];
        for index in (0..WORDS).rev() {
            let Some(source) = index.checked_sub(word_shift) else {
                continue;
            };
            let mut value = self.bits.get(source).copied().unwrap_or(0) << bit_shift;
            if bit_shift > 0
                && let Some(lower) = source.checked_sub(1)
            {
                value |= self.bits.get(lower).copied().unwrap_or(0) >> (64 - bit_shift);
            }
            if let Some(slot) = out.get_mut(index) {
                *slot = value;
            }
        }
        self.bits = out;
    }

    fn get(&self, offset: u64) -> bool {
        let word = usize::try_from(offset / 64).unwrap_or(WORDS);
        let bit = offset % 64;
        self.bits.get(word).is_some_and(|w| w & (1u64 << bit) != 0)
    }

    fn set(&mut self, offset: u64) {
        let word = usize::try_from(offset / 64).unwrap_or(WORDS);
        let bit = offset % 64;
        if let Some(slot) = self.bits.get_mut(word) {
            *slot |= 1u64 << bit;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fresh_window_accepts_the_first_counter() {
        let mut w = ReplayWindow::new();
        assert!(w.accept(1));
    }

    #[test]
    fn zero_is_never_accepted() {
        let mut w = ReplayWindow::new();
        assert!(!w.accept(0));
    }

    #[test]
    fn a_repeat_is_rejected() {
        let mut w = ReplayWindow::new();
        assert!(w.accept(5));
        assert!(!w.accept(5), "o mesmo contador não pode passar duas vezes");
    }

    #[test]
    fn in_order_counters_all_pass() {
        let mut w = ReplayWindow::new();
        for c in 1..=1000 {
            assert!(w.accept(c), "contador {c} deveria passar");
        }
    }

    #[test]
    fn out_of_order_within_the_window_is_accepted_once() {
        let mut w = ReplayWindow::new();
        assert!(w.accept(100));
        assert!(w.accept(98), "atrasado mas dentro da janela");
        assert!(w.accept(99));
        assert!(!w.accept(98), "mas só uma vez");
        assert!(!w.accept(100));
    }

    #[test]
    fn a_counter_too_old_for_the_window_is_rejected() {
        let mut w = ReplayWindow::new();
        assert!(w.accept(5000));
        assert!(!w.accept(1), "5000 - 1 passa da janela de 2048");
        assert!(w.accept(5000 - 2047), "na borda da janela ainda entra");
    }

    #[test]
    fn a_big_jump_forward_resets_and_accepts() {
        let mut w = ReplayWindow::new();
        assert!(w.accept(10));
        assert!(w.accept(10_000), "salto maior que a janela");
        assert!(!w.accept(10), "e o antigo não volta a passar");
    }

    #[test]
    fn the_window_edge_shifts_correctly_across_words() {
        // Aceita alguns, salta 130 bits (mais de dois words), e confere que os antigos certos
        // continuam bloqueados e um novo atrasado ainda entra.
        let mut w = ReplayWindow::new();
        assert!(w.accept(1));
        assert!(w.accept(3));
        assert!(w.accept(133));
        assert!(!w.accept(1), "1 continua visto após o deslocamento");
        assert!(!w.accept(3));
        assert!(w.accept(2), "2 nunca foi visto e ainda cabe");
    }
}
