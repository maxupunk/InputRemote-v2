//! Estado dos oito modificadores, num byte.
//!
//! Os modificadores viajam **de duas formas ao mesmo tempo**: como evento de tecla, no
//! canal de entrada confiável, e como estado, em toda mensagem de entrada
//! (`docs/03-protocolo.md` §5).
//!
//! A redundância custa 1 byte por mensagem e elimina a categoria inteira de bugs de
//! "o Ctrl ficou preso". Quando o estado recebido divergir do estado aplicado, o cliente
//! corrige antes de processar o evento — e a correção é idempotente.
//!
//! O layout de bits é o mesmo do byte de modificadores de um relatório HID de teclado, o
//! que torna a conversão nos backends trivial e sem tabela.

use serde::{Deserialize, Serialize};

use super::HidUsage;

/// Conjunto de modificadores pressionados.
#[derive(Clone, Copy, PartialEq, Eq, Default, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Modifiers(u8);

impl Modifiers {
    /// Nenhum modificador pressionado.
    pub const NONE: Self = Self(0);

    /// Ctrl esquerdo.
    pub const LEFT_CTRL: Self = Self(1 << 0);
    /// Shift esquerdo.
    pub const LEFT_SHIFT: Self = Self(1 << 1);
    /// Alt esquerdo.
    pub const LEFT_ALT: Self = Self(1 << 2);
    /// Meta esquerdo.
    pub const LEFT_GUI: Self = Self(1 << 3);
    /// Ctrl direito.
    pub const RIGHT_CTRL: Self = Self(1 << 4);
    /// Shift direito.
    pub const RIGHT_SHIFT: Self = Self(1 << 5);
    /// Alt direito.
    pub const RIGHT_ALT: Self = Self(1 << 6);
    /// Meta direito.
    pub const RIGHT_GUI: Self = Self(1 << 7);

    /// O byte cru, no layout de relatório HID.
    #[must_use]
    pub const fn bits(self) -> u8 {
        self.0
    }

    /// Constrói a partir do byte cru de um relatório HID.
    ///
    /// Todos os 256 valores são válidos — os oito bits têm significado —, então não há
    /// caminho de erro.
    #[must_use]
    pub const fn from_bits(bits: u8) -> Self {
        Self(bits)
    }

    /// Se todos os modificadores de `other` estão presentes.
    #[must_use]
    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    /// União dos dois conjuntos.
    #[must_use]
    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    /// `self` sem os modificadores de `other`.
    #[must_use]
    pub const fn without(self, other: Self) -> Self {
        Self(self.0 & !other.0)
    }

    /// Quais modificadores existem em `self` e não em `other`.
    ///
    /// É a metade "precisa pressionar" de uma reconciliação de estado.
    #[must_use]
    pub const fn missing_from(self, other: Self) -> Self {
        Self(self.0 & !other.0)
    }

    /// Se nenhum modificador está pressionado.
    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// O modificador correspondente a uma tecla, se ela for um modificador.
    ///
    /// Retorna `None` para qualquer tecla comum. É a única ponte entre
    /// [`HidUsage`] e este tipo, e ela é sem tabela por causa do layout escolhido.
    #[must_use]
    pub const fn from_usage(usage: HidUsage) -> Option<Self> {
        if usage.is_modifier() {
            // 0xE0..=0xE7 → bits 0..=7, na mesma ordem do relatório HID.
            let index = usage.get() - HidUsage::LEFT_CTRL.get();
            Some(Self(1u8 << index))
        } else {
            None
        }
    }

    /// Aplica um evento de tecla a este conjunto.
    ///
    /// Teclas comuns não alteram nada. Retorna o conjunto resultante, sem mutar — este é um
    /// tipo de valor, e o estado de quem o guarda é responsabilidade de `ir-session`.
    #[must_use]
    pub const fn applying(self, usage: HidUsage, pressed: bool) -> Self {
        match Self::from_usage(usage) {
            Some(bit) if pressed => self.union(bit),
            Some(bit) => self.without(bit),
            None => self,
        }
    }
}

impl core::fmt::Debug for Modifiers {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        // Nomes de modificador não são conteúdo digitado: Ctrl e Shift podem aparecer em
        // log de diagnóstico sem revelar a senha.
        const NAMES: [(u8, &str); 8] = [
            (1 << 0, "LCtrl"),
            (1 << 1, "LShift"),
            (1 << 2, "LAlt"),
            (1 << 3, "LGui"),
            (1 << 4, "RCtrl"),
            (1 << 5, "RShift"),
            (1 << 6, "RAlt"),
            (1 << 7, "RGui"),
        ];
        if self.0 == 0 {
            return f.write_str("Modifiers(nenhum)");
        }
        f.write_str("Modifiers(")?;
        let mut first = true;
        for (bit, name) in NAMES {
            if self.0 & bit != 0 {
                if !first {
                    f.write_str("+")?;
                }
                f.write_str(name)?;
                first = false;
            }
        }
        f.write_str(")")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: [Modifiers; 8] = [
        Modifiers::LEFT_CTRL,
        Modifiers::LEFT_SHIFT,
        Modifiers::LEFT_ALT,
        Modifiers::LEFT_GUI,
        Modifiers::RIGHT_CTRL,
        Modifiers::RIGHT_SHIFT,
        Modifiers::RIGHT_ALT,
        Modifiers::RIGHT_GUI,
    ];

    const USAGES: [HidUsage; 8] = [
        HidUsage::LEFT_CTRL,
        HidUsage::LEFT_SHIFT,
        HidUsage::LEFT_ALT,
        HidUsage::LEFT_GUI,
        HidUsage::RIGHT_CTRL,
        HidUsage::RIGHT_SHIFT,
        HidUsage::RIGHT_ALT,
        HidUsage::RIGHT_GUI,
    ];

    #[test]
    fn usage_maps_to_the_matching_bit_in_hid_report_order() {
        for (usage, expected) in USAGES.iter().zip(ALL) {
            assert_eq!(Modifiers::from_usage(*usage), Some(expected));
        }
    }

    #[test]
    fn ordinary_keys_map_to_no_modifier() {
        for raw in [0x04u16, 0x28, 0x39, 0x65] {
            assert_eq!(Modifiers::from_usage(HidUsage(raw)), None);
        }
    }

    #[test]
    fn applying_an_ordinary_key_changes_nothing() {
        let state = Modifiers::LEFT_CTRL;
        assert_eq!(state.applying(HidUsage(0x04), true), state);
        assert_eq!(state.applying(HidUsage(0x04), false), state);
    }

    #[test]
    fn press_then_release_returns_to_the_original_state() {
        for usage in USAGES {
            let start = Modifiers::NONE;
            let pressed = start.applying(usage, true);
            assert!(!pressed.is_empty());
            let released = pressed.applying(usage, false);
            assert_eq!(released, start, "{usage} não voltou ao estado inicial");
        }
    }

    #[test]
    fn releasing_a_modifier_never_touches_the_others() {
        let both = Modifiers::LEFT_CTRL.union(Modifiers::RIGHT_SHIFT);
        let after = both.applying(HidUsage::LEFT_CTRL, false);
        assert_eq!(after, Modifiers::RIGHT_SHIFT);
    }

    #[test]
    fn reconciliation_is_symmetric_and_idempotent() {
        let desired = Modifiers::LEFT_CTRL.union(Modifiers::LEFT_ALT);
        let applied = Modifiers::LEFT_ALT.union(Modifiers::RIGHT_GUI);

        let to_press = desired.missing_from(applied);
        let to_release = applied.missing_from(desired);
        assert_eq!(to_press, Modifiers::LEFT_CTRL);
        assert_eq!(to_release, Modifiers::RIGHT_GUI);

        let reconciled = applied.union(to_press).without(to_release);
        assert_eq!(reconciled, desired);

        // Idempotência: reconciliar de novo não muda nada.
        let again = reconciled
            .union(desired.missing_from(reconciled))
            .without(reconciled.missing_from(desired));
        assert_eq!(again, desired);
    }

    #[test]
    fn every_bit_pattern_survives_a_bits_round_trip() {
        for bits in 0u8..=255 {
            assert_eq!(Modifiers::from_bits(bits).bits(), bits);
        }
    }

    #[test]
    fn debug_shows_modifier_names_but_never_typed_content() {
        assert_eq!(format!("{:?}", Modifiers::NONE), "Modifiers(nenhum)");
        let combo = Modifiers::LEFT_CTRL.union(Modifiers::LEFT_SHIFT);
        assert_eq!(format!("{combo:?}"), "Modifiers(LCtrl+LShift)");
    }

    #[test]
    fn contains_requires_every_requested_bit() {
        let combo = Modifiers::LEFT_CTRL.union(Modifiers::LEFT_ALT);
        assert!(combo.contains(Modifiers::LEFT_CTRL));
        assert!(combo.contains(combo));
        assert!(!combo.contains(Modifiers::RIGHT_CTRL));
        assert!(combo.contains(Modifiers::NONE));
    }
}
