//! A tecla física, como HID Usage ID.
//!
//! No fio não viaja caractere, não viaja código virtual do Windows e não viaja *keysym* do
//! X. Viaja o **HID Usage ID da Usage Page 0x07** — a identidade da tecla física, tal como
//! o próprio teclado a reporta.
//!
//! A razão é o requisito da tela de login (`docs/03-protocolo.md` §5): quem digita a senha
//! precisa que o teclado se comporte como o teclado **da máquina controlada**, com o layout
//! dela. Mandar o caractere resolvido pela origem produziria a senha errada em qualquer par
//! de layouts diferentes.
//!
//! A tradução para scancode PS/2 (Windows) e para `KEY_*` do evdev (Linux) acontece nos
//! backends de `ir-input`, nunca aqui.

use serde::{Deserialize, Serialize};

/// Uma tecla física, pelo seu HID Usage ID na Usage Page 0x07.
///
/// A página é implícita. Teclas de mídia e de energia vivem em outras páginas (0x0C, 0x01)
/// e ficam fora desta versão do protocolo — acrescentá-las exige incremento de
/// `version::CURRENT`, porque muda o significado do campo.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct HidUsage(pub u16);

/// As faixas de teclas que o produto carrega, em HID Usage da página 0x07.
///
/// É o contrato entre os dois backends de `ir-input`, e é contra ele que os dois são testados: uma
/// tecla que só um lado saiba traduzir **some** na travessia naquele sentido, sem erro e sem
/// registro. Foi assim que o `PrintScreen` e o teclado numérico inteiro deixaram de sair do
/// Windows, e ninguém viu até alguém apertar.
///
/// Cobre o teclado de 104 teclas e as três que o ABNT2 brasileiro tem a mais. De fora ficam as
/// teclas dos teclados japonês e coreano, e o `Pause`, que o Windows manda como uma sequência de
/// três scancodes. Teclas de mídia e de energia vivem em outras páginas, e acrescentá-las muda o
/// protocolo (ver [`HidUsage`]).
const FAIXAS_DO_TECLADO: &[(u16, u16)] = &[
    // Letras, dígitos, pontuação, Enter, Esc, Backspace, Tab, espaço.
    (0x04, 0x31),
    // Caps Lock, F1–F12, PrintScreen, Scroll Lock, Pause, o bloco de navegação, Num Lock e o
    // teclado numérico inteiro.
    (0x33, 0x63),
    // A `\ |` dos teclados não americanos e a tecla de menu de contexto.
    (0x64, 0x65),
    // A vírgula do teclado numérico e a `/ ? °`, as outras duas do ABNT2.
    (0x85, 0x85),
    (0x87, 0x87),
    // Os oito modificadores.
    (0xE0, 0xE7),
];

/// Todas as teclas do contrato, uma a uma.
pub fn teclado_completo() -> impl Iterator<Item = HidUsage> {
    FAIXAS_DO_TECLADO
        .iter()
        .flat_map(|(primeira, ultima)| (*primeira..=*ultima).map(HidUsage))
}

impl HidUsage {
    /// Menor valor com significado na página 0x07.
    pub const MIN: Self = Self(0x04);
    /// Maior valor atribuído na página 0x07 (Right GUI).
    pub const MAX: Self = Self(0xE7);

    // Modificadores. São os únicos nomeados aqui porque o protocolo os trata de forma
    // especial: viajam como evento e como estado ao mesmo tempo (ver `super::Modifiers`).
    /// Ctrl esquerdo.
    pub const LEFT_CTRL: Self = Self(0xE0);
    /// Shift esquerdo.
    pub const LEFT_SHIFT: Self = Self(0xE1);
    /// Alt esquerdo.
    pub const LEFT_ALT: Self = Self(0xE2);
    /// Meta/Win/Super esquerdo.
    pub const LEFT_GUI: Self = Self(0xE3);
    /// Ctrl direito.
    pub const RIGHT_CTRL: Self = Self(0xE4);
    /// Shift direito.
    pub const RIGHT_SHIFT: Self = Self(0xE5);
    /// Alt direito (`AltGr` em muitos layouts).
    pub const RIGHT_ALT: Self = Self(0xE6);
    /// Meta/Win/Super direito.
    pub const RIGHT_GUI: Self = Self(0xE7);

    /// Os oito modificadores, na ordem do relatório HID — a mesma dos bits de `super::Modifiers`.
    pub const MODIFIERS: [Self; 8] = [
        Self::LEFT_CTRL,
        Self::LEFT_SHIFT,
        Self::LEFT_ALT,
        Self::LEFT_GUI,
        Self::RIGHT_CTRL,
        Self::RIGHT_SHIFT,
        Self::RIGHT_ALT,
        Self::RIGHT_GUI,
    ];

    /// Se o valor está na faixa atribuída da página 0x07.
    ///
    /// Um valor fora da faixa não é recusado pelo decodificador — ele é **descartado pelo
    /// injetor**, que não tem tradução para ele. Recusar derrubaria o enlace por causa de
    /// uma tecla exótica num teclado incomum, o que seria pior do que ignorá-la.
    #[must_use]
    pub const fn is_assigned(self) -> bool {
        self.0 >= Self::MIN.0 && self.0 <= Self::MAX.0
    }

    /// Se esta tecla é um dos oito modificadores.
    #[must_use]
    pub const fn is_modifier(self) -> bool {
        self.0 >= Self::LEFT_CTRL.0 && self.0 <= Self::RIGHT_GUI.0
    }

    /// O número cru, para a tabela de tradução do backend.
    #[must_use]
    pub const fn get(self) -> u16 {
        self.0
    }
}

impl core::fmt::Display for HidUsage {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        // Deliberadamente numérico, nunca o nome nem o caractere da tecla: esta
        // implementação pode acabar num log, e o que se digita não vai para log
        // (`docs/04-seguranca.md` §7).
        write!(f, "usage:{:#04x}", self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_eight_modifiers_are_contiguous_and_recognised() {
        for (index, usage) in HidUsage::MODIFIERS.iter().enumerate() {
            assert!(usage.is_modifier(), "{usage} deveria ser modificador");
            assert!(usage.is_assigned());
            let expected = 0xE0 + u16::try_from(index).unwrap();
            assert_eq!(usage.get(), expected);
        }
    }

    #[test]
    fn ordinary_keys_are_not_modifiers() {
        // 0x04 é 'a' na página 0x07; 0x39 é Caps Lock, que não é modificador de estado HID.
        for raw in [0x04u16, 0x1D, 0x28, 0x39, 0x3A, 0x65] {
            assert!(!HidUsage(raw).is_modifier());
        }
    }

    #[test]
    fn out_of_range_values_are_unassigned_but_not_an_error() {
        for raw in [0x00u16, 0x01, 0x02, 0x03, 0xE8, 0xFF, 0xFFFF] {
            assert!(
                !HidUsage(raw).is_assigned(),
                "{raw:#x} não deveria estar atribuído"
            );
        }
    }

    #[test]
    fn display_never_reveals_which_key_was_pressed() {
        // A forma é sempre `usage:0xNN` — prefixo fixo mais hexadecimal. Nada que dependa
        // de qual tecla é, e nada que possa vazar o caractere digitado para um log.
        for raw in [0x04u16, 0x1D, 0x28, 0xE0, 0xFF] {
            let rendered = HidUsage(raw).to_string();
            let digits = rendered.strip_prefix("usage:0x").expect("prefixo fixo");
            assert!(
                digits.chars().all(|c| c.is_ascii_hexdigit()),
                "{rendered} deveria ser só hexadecimal após o prefixo"
            );
            assert_eq!(
                u16::from_str_radix(digits, 16).unwrap(),
                raw,
                "o número exibido é o próprio usage, sem tradução para caractere"
            );
        }
    }
}
