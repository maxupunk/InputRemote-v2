//! Tradução de HID Usage (página 0x07) para código de tecla do Linux (`evdev`).
//!
//! A chave física viaja como HID Usage ([03, §5](../../../docs/03-protocolo.md)); o caractere é
//! decidido pelo layout da máquina **controlada**, então aqui só se mapeia a posição física.
//!
//! Cobre o teclado comum — o suficiente para digitar uma senha e navegar. Teclas fora do mapa
//! são ignoradas, não derrubam a sessão.

#![allow(unreachable_pub)]

use evdev::Key;
use ir_proto::input::HidUsage;

/// A tabela HID Usage → tecla do Linux. Uma linha por tecla, para nenhuma função ficar longa.
const MAP: &[(u16, Key)] = &[
    (0x04, Key::KEY_A),
    (0x05, Key::KEY_B),
    (0x06, Key::KEY_C),
    (0x07, Key::KEY_D),
    (0x08, Key::KEY_E),
    (0x09, Key::KEY_F),
    (0x0A, Key::KEY_G),
    (0x0B, Key::KEY_H),
    (0x0C, Key::KEY_I),
    (0x0D, Key::KEY_J),
    (0x0E, Key::KEY_K),
    (0x0F, Key::KEY_L),
    (0x10, Key::KEY_M),
    (0x11, Key::KEY_N),
    (0x12, Key::KEY_O),
    (0x13, Key::KEY_P),
    (0x14, Key::KEY_Q),
    (0x15, Key::KEY_R),
    (0x16, Key::KEY_S),
    (0x17, Key::KEY_T),
    (0x18, Key::KEY_U),
    (0x19, Key::KEY_V),
    (0x1A, Key::KEY_W),
    (0x1B, Key::KEY_X),
    (0x1C, Key::KEY_Y),
    (0x1D, Key::KEY_Z),
    (0x1E, Key::KEY_1),
    (0x1F, Key::KEY_2),
    (0x20, Key::KEY_3),
    (0x21, Key::KEY_4),
    (0x22, Key::KEY_5),
    (0x23, Key::KEY_6),
    (0x24, Key::KEY_7),
    (0x25, Key::KEY_8),
    (0x26, Key::KEY_9),
    (0x27, Key::KEY_0),
    (0x28, Key::KEY_ENTER),
    (0x29, Key::KEY_ESC),
    (0x2A, Key::KEY_BACKSPACE),
    (0x2B, Key::KEY_TAB),
    (0x2C, Key::KEY_SPACE),
    (0x2D, Key::KEY_MINUS),
    (0x2E, Key::KEY_EQUAL),
    (0x2F, Key::KEY_LEFTBRACE),
    (0x30, Key::KEY_RIGHTBRACE),
    (0x31, Key::KEY_BACKSLASH),
    (0x33, Key::KEY_SEMICOLON),
    (0x34, Key::KEY_APOSTROPHE),
    (0x35, Key::KEY_GRAVE),
    (0x36, Key::KEY_COMMA),
    (0x37, Key::KEY_DOT),
    (0x38, Key::KEY_SLASH),
    (0x39, Key::KEY_CAPSLOCK),
    (0x3A, Key::KEY_F1),
    (0x3B, Key::KEY_F2),
    (0x3C, Key::KEY_F3),
    (0x3D, Key::KEY_F4),
    (0x3E, Key::KEY_F5),
    (0x3F, Key::KEY_F6),
    (0x40, Key::KEY_F7),
    (0x41, Key::KEY_F8),
    (0x42, Key::KEY_F9),
    (0x43, Key::KEY_F10),
    (0x44, Key::KEY_F11),
    (0x45, Key::KEY_F12),
    (0x46, Key::KEY_SYSRQ),
    (0x47, Key::KEY_SCROLLLOCK),
    (0x48, Key::KEY_PAUSE),
    (0x49, Key::KEY_INSERT),
    (0x4A, Key::KEY_HOME),
    (0x4B, Key::KEY_PAGEUP),
    (0x4C, Key::KEY_DELETE),
    (0x4D, Key::KEY_END),
    (0x4E, Key::KEY_PAGEDOWN),
    (0x4F, Key::KEY_RIGHT),
    (0x50, Key::KEY_LEFT),
    (0x51, Key::KEY_DOWN),
    (0x52, Key::KEY_UP),
    (0x53, Key::KEY_NUMLOCK),
    (0x54, Key::KEY_KPSLASH),
    (0x55, Key::KEY_KPASTERISK),
    (0x56, Key::KEY_KPMINUS),
    (0x57, Key::KEY_KPPLUS),
    (0x58, Key::KEY_KPENTER),
    (0x59, Key::KEY_KP1),
    (0x5A, Key::KEY_KP2),
    (0x5B, Key::KEY_KP3),
    (0x5C, Key::KEY_KP4),
    (0x5D, Key::KEY_KP5),
    (0x5E, Key::KEY_KP6),
    (0x5F, Key::KEY_KP7),
    (0x60, Key::KEY_KP8),
    (0x61, Key::KEY_KP9),
    (0x62, Key::KEY_KP0),
    (0x63, Key::KEY_KPDOT),
    (0x65, Key::KEY_COMPOSE),
    // As teclas do ABNT2 que o teclado americano não tem (ver `windows::scancode`).
    (0x64, Key::KEY_102ND),
    (0x85, Key::KEY_KPCOMMA),
    (0x87, Key::KEY_RO),
    (0xE0, Key::KEY_LEFTCTRL),
    (0xE1, Key::KEY_LEFTSHIFT),
    (0xE2, Key::KEY_LEFTALT),
    (0xE3, Key::KEY_LEFTMETA),
    (0xE4, Key::KEY_RIGHTCTRL),
    (0xE5, Key::KEY_RIGHTSHIFT),
    (0xE6, Key::KEY_RIGHTALT),
    (0xE7, Key::KEY_RIGHTMETA),
];

/// O código de tecla do Linux para um HID Usage, se houver correspondência.
#[must_use]
pub fn hid_to_key(usage: HidUsage) -> Option<Key> {
    MAP.iter()
        .find(|(hid, _)| *hid == usage.get())
        .map(|(_, key)| *key)
}

/// Todas as teclas que o dispositivo virtual precisa declarar como capazes de emitir.
///
/// O `uinput` só emite uma tecla que o dispositivo declarou na criação; declarar a faixa toda de
/// uma vez é mais simples que descobrir sob demanda, e o dispositivo é criado uma vez na subida.
#[must_use]
pub fn all_keys() -> Vec<Key> {
    MAP.iter().map(|(_, key)| *key).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A lista de `ir-proto` é o contrato entre os dois backends: uma tecla que só um deles saiba
    /// traduzir some na travessia naquele sentido, calada. Foi o caso do `PrintScreen` e do teclado
    /// numérico, que o Windows não sabia capturar.
    #[test]
    fn o_linux_injeta_todas_as_teclas_do_contrato() {
        for usage in ir_proto::input::teclado_completo() {
            assert!(
                hid_to_key(usage).is_some(),
                "HID {:#04x} sem tecla no Linux",
                usage.get()
            );
        }
    }
}
