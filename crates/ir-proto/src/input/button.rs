//! Botões do ponteiro.
//!
//! Conjunto fechado. Um botão desconhecido não derruba o enlace — ele é ignorado pelo
//! injetor, porque um mouse com nove botões não é motivo para encerrar uma sessão.

use serde::{Deserialize, Serialize};

/// Um botão do ponteiro.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[repr(u8)]
pub enum Button {
    /// Botão principal.
    Left = 0,
    /// Botão de contexto.
    Right = 1,
    /// Roda pressionada.
    Middle = 2,
    /// Lateral "voltar" (X1 no Windows, `BTN_SIDE` no evdev).
    Back = 3,
    /// Lateral "avançar" (X2 no Windows, `BTN_EXTRA` no evdev).
    Forward = 4,
}

impl Button {
    /// Todos os botões, para varreduras e testes exaustivos.
    pub const ALL: [Self; 5] = [
        Self::Left,
        Self::Right,
        Self::Middle,
        Self::Back,
        Self::Forward,
    ];

    /// Posição deste botão no bitmap de [`Buttons`].
    #[must_use]
    pub const fn bit(self) -> u8 {
        1u8 << (self as u8)
    }

    /// Nome estável, para diagnóstico.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Left => "esquerdo",
            Self::Right => "direito",
            Self::Middle => "meio",
            Self::Back => "voltar",
            Self::Forward => "avançar",
        }
    }
}

impl core::fmt::Display for Button {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.name())
    }
}

/// Conjunto de botões pressionados.
///
/// Existe pelo mesmo motivo que [`super::Modifiers`]: o `StateSnapshot` precisa carregar o
/// estado completo para que a reconciliação seja idempotente
/// (`docs/03-protocolo.md` §7).
#[derive(Clone, Copy, PartialEq, Eq, Default, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Buttons(u8);

impl Buttons {
    /// Nenhum botão pressionado.
    pub const NONE: Self = Self(0);

    /// O byte cru.
    #[must_use]
    pub const fn bits(self) -> u8 {
        self.0
    }

    /// Constrói a partir do byte cru.
    ///
    /// Bits acima do último botão conhecido são preservados mas nunca consultados — assim
    /// um par mais novo, com mais botões, não faz este decodificador falhar.
    #[must_use]
    pub const fn from_bits(bits: u8) -> Self {
        Self(bits)
    }

    /// Se este botão está pressionado.
    #[must_use]
    pub const fn contains(self, button: Button) -> bool {
        self.0 & button.bit() != 0
    }

    /// Aplica um evento de botão, sem mutar.
    #[must_use]
    pub const fn applying(self, button: Button, pressed: bool) -> Self {
        if pressed {
            Self(self.0 | button.bit())
        } else {
            Self(self.0 & !button.bit())
        }
    }

    /// Se nenhum botão está pressionado.
    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// Quais botões existem em `self` e não em `other`.
    #[must_use]
    pub const fn missing_from(self, other: Self) -> Self {
        Self(self.0 & !other.0)
    }

    /// Itera os botões conhecidos que estão pressionados.
    pub fn iter(self) -> impl Iterator<Item = Button> {
        Button::ALL.into_iter().filter(move |b| self.contains(*b))
    }
}

impl core::fmt::Debug for Buttons {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        if self.is_empty() {
            return f.write_str("Buttons(nenhum)");
        }
        f.write_str("Buttons(")?;
        let mut first = true;
        for button in self.iter() {
            if !first {
                f.write_str("+")?;
            }
            f.write_str(button.name())?;
            first = false;
        }
        f.write_str(")")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_button_has_a_distinct_bit() {
        let mut seen = 0u8;
        for button in Button::ALL {
            assert_eq!(seen & button.bit(), 0, "{button} colide com outro botão");
            seen |= button.bit();
        }
        assert_eq!(seen.count_ones(), 5);
    }

    #[test]
    fn press_then_release_returns_to_the_original_state() {
        for button in Button::ALL {
            let pressed = Buttons::NONE.applying(button, true);
            assert!(pressed.contains(button));
            assert_eq!(pressed.applying(button, false), Buttons::NONE);
        }
    }

    #[test]
    fn releasing_one_button_never_touches_the_others() {
        let both = Buttons::NONE
            .applying(Button::Left, true)
            .applying(Button::Right, true);
        let after = both.applying(Button::Left, false);
        assert!(!after.contains(Button::Left));
        assert!(after.contains(Button::Right));
    }

    #[test]
    fn unknown_high_bits_survive_but_are_never_reported() {
        let exotic = Buttons::from_bits(0b1010_0000);
        assert_eq!(
            exotic.bits(),
            0b1010_0000,
            "bits de botões futuros são preservados"
        );
        assert_eq!(
            exotic.iter().count(),
            0,
            "e nunca reportados como botão conhecido"
        );
    }

    #[test]
    fn reconciliation_finds_what_to_press_and_what_to_release() {
        let desired = Buttons::NONE
            .applying(Button::Left, true)
            .applying(Button::Middle, true);
        let applied = Buttons::NONE
            .applying(Button::Middle, true)
            .applying(Button::Right, true);

        let to_press: Vec<_> = desired.missing_from(applied).iter().collect();
        let to_release: Vec<_> = applied.missing_from(desired).iter().collect();
        assert_eq!(to_press, vec![Button::Left]);
        assert_eq!(to_release, vec![Button::Right]);
    }

    #[test]
    fn debug_lists_pressed_buttons() {
        assert_eq!(format!("{:?}", Buttons::NONE), "Buttons(nenhum)");
        let combo = Buttons::NONE
            .applying(Button::Left, true)
            .applying(Button::Back, true);
        assert_eq!(format!("{combo:?}"), "Buttons(esquerdo+voltar)");
    }
}
