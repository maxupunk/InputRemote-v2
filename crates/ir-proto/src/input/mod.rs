//! Vocabulário de entrada: teclas, modificadores, botões, ponteiro e roda.
//!
//! Tudo aqui é tipo de valor: `Copy` quando cabe, sem estado interno escondido, sem
//! alocação no caminho quente (a única exceção é [`PressedKeys`], que é limitada e só
//! aparece no `StateSnapshot`, fora do caminho quente).
//!
//! Nenhum tipo deste módulo tem `Display` que revele o que foi digitado — regra de
//! `docs/04-seguranca.md` §7. `Debug` de modificador e de botão mostra nomes porque
//! "Ctrl" e "esquerdo" não são conteúdo de senha; `Debug` de tecla mostra o número.

mod button;
mod event;
mod hid;
mod modifiers;
mod pressed;

pub use button::{Button, Buttons};
pub use event::{PointerDelta, PointerPosition, WheelDelta};
pub use hid::{HidUsage, teclado_completo};
pub use modifiers::Modifiers;
pub use pressed::PressedKeys;

/// Estado completo de entrada num instante.
///
/// É o conteúdo do `StateSnapshot` de `docs/03-protocolo.md` §7, e a razão de ele existir:
/// reconciliar um estado completo é idempotente, enquanto reaplicar uma sequência de
/// eventos não é. Qualquer perda no caminho é corrigida pelo snapshot seguinte,
/// independentemente da causa.
#[derive(Debug, Clone, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub struct InputState {
    /// Teclas comuns pressionadas.
    pub keys: PressedKeys,
    /// Modificadores pressionados.
    pub modifiers: Modifiers,
    /// Botões do ponteiro pressionados.
    pub buttons: Buttons,
}

impl InputState {
    /// Nada pressionado.
    #[must_use]
    pub fn released() -> Self {
        Self::default()
    }

    /// Se nada está pressionado.
    #[must_use]
    pub fn is_released(&self) -> bool {
        self.keys.is_empty() && self.modifiers.is_empty() && self.buttons.is_empty()
    }

    /// Aplica um evento de tecla, mantendo teclas e modificadores coerentes.
    ///
    /// Um modificador é registrado nos **dois** lugares: no bitmap de [`Modifiers`], que é
    /// o que viaja em toda mensagem de entrada, e no conjunto de teclas, que é o que o
    /// injetor precisa para soltar tudo. Manter os dois em sincronia num só lugar é o que
    /// evita o estado inconsistente que produz modificador preso.
    pub fn apply_key(&mut self, usage: HidUsage, pressed: bool) {
        self.modifiers = self.modifiers.applying(usage, pressed);
        self.keys.apply(usage, pressed);
    }

    /// Aplica um evento de botão.
    pub fn apply_button(&mut self, button: Button, pressed: bool) {
        self.buttons = self.buttons.applying(button, pressed);
    }

    /// Solta tudo.
    ///
    /// Chamado em toda falha, em toda queda de enlace e em todo encerramento
    /// (`docs/02-arquitetura.md` §8). É a operação mais importante do produto: um serviço
    /// que morre com `Ctrl` pressionado deixa a máquina inutilizável.
    pub fn release_all(&mut self) {
        self.keys.clear();
        self.modifiers = Modifiers::NONE;
        self.buttons = Buttons::NONE;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_modifier_press_is_recorded_in_both_places() {
        let mut state = InputState::released();
        state.apply_key(HidUsage::LEFT_CTRL, true);
        assert!(
            state.modifiers.contains(Modifiers::LEFT_CTRL),
            "no bitmap que viaja"
        );
        assert!(
            state.keys.contains(HidUsage::LEFT_CTRL),
            "e no conjunto que o injetor solta"
        );
    }

    #[test]
    fn a_modifier_release_clears_both_places() {
        let mut state = InputState::released();
        state.apply_key(HidUsage::LEFT_CTRL, true);
        state.apply_key(HidUsage::LEFT_CTRL, false);
        assert!(state.modifiers.is_empty());
        assert!(!state.keys.contains(HidUsage::LEFT_CTRL));
        assert!(state.is_released());
    }

    #[test]
    fn an_ordinary_key_never_touches_the_modifier_bitmap() {
        let mut state = InputState::released();
        state.apply_key(HidUsage(0x04), true);
        assert!(state.modifiers.is_empty());
        assert!(state.keys.contains(HidUsage(0x04)));
    }

    #[test]
    fn release_all_clears_everything_at_once() {
        let mut state = InputState::released();
        state.apply_key(HidUsage::LEFT_SHIFT, true);
        state.apply_key(HidUsage(0x04), true);
        state.apply_button(Button::Left, true);
        assert!(!state.is_released());

        state.release_all();
        assert!(state.is_released());
        assert!(state.keys.is_empty());
        assert!(state.modifiers.is_empty());
        assert!(state.buttons.is_empty());
    }

    #[test]
    fn release_all_is_idempotent() {
        let mut state = InputState::released();
        state.apply_key(HidUsage::LEFT_ALT, true);
        state.release_all();
        let once = state.clone();
        state.release_all();
        assert_eq!(state, once);
    }

    #[test]
    fn a_late_key_up_after_release_all_is_harmless() {
        // Cenário real: o enlace cai, tudo é solto, e o `KeyUp` original chega depois.
        let mut state = InputState::released();
        state.apply_key(HidUsage::LEFT_CTRL, true);
        state.release_all();
        state.apply_key(HidUsage::LEFT_CTRL, false);
        assert!(
            state.is_released(),
            "o KeyUp atrasado não pode desbalancear o estado"
        );
    }
}
