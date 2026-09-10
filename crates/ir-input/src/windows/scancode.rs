//! Tradução entre HID Usage (página 0x07) e scancode do conjunto 1 do Windows.
//!
//! A captura recebe scancode do gancho e o converte em HID Usage; a injeção faz o caminho
//! inverso com `KEYEVENTF_SCANCODE` ([05, §4.1](../../../docs/05-windows.md)). Injetar por
//! scancode faz o layout da máquina **controlada** decidir o caractere — o requisito da tela de
//! login.
//!
//! O bit `estendido` distingue teclas que compartilham scancode (Ctrl direito vs esquerdo, as
//! setas do bloco de navegação vs o teclado numérico): no gancho vem por `LLKHF_EXTENDED`, na
//! injeção sai por `KEYEVENTF_EXTENDEDKEY`.

#![allow(unreachable_pub)]

use ir_proto::input::HidUsage;

/// A tabela HID ↔ scancode. Cada linha: HID Usage, scancode do conjunto 1, se é estendido.
///
/// Cobre o teclado comum — o suficiente para digitar uma senha e navegar.
const MAP: &[(u16, u16, bool)] = &[
    // Letras (posição física; o caractere é do layout do outro lado).
    (0x04, 0x1E, false),
    (0x05, 0x30, false),
    (0x06, 0x2E, false),
    (0x07, 0x20, false),
    (0x08, 0x12, false),
    (0x09, 0x21, false),
    (0x0A, 0x22, false),
    (0x0B, 0x23, false),
    (0x0C, 0x17, false),
    (0x0D, 0x24, false),
    (0x0E, 0x25, false),
    (0x0F, 0x26, false),
    (0x10, 0x32, false),
    (0x11, 0x31, false),
    (0x12, 0x18, false),
    (0x13, 0x19, false),
    (0x14, 0x10, false),
    (0x15, 0x13, false),
    (0x16, 0x1F, false),
    (0x17, 0x14, false),
    (0x18, 0x16, false),
    (0x19, 0x2F, false),
    (0x1A, 0x11, false),
    (0x1B, 0x2D, false),
    (0x1C, 0x15, false),
    (0x1D, 0x2C, false),
    // Dígitos.
    (0x1E, 0x02, false),
    (0x1F, 0x03, false),
    (0x20, 0x04, false),
    (0x21, 0x05, false),
    (0x22, 0x06, false),
    (0x23, 0x07, false),
    (0x24, 0x08, false),
    (0x25, 0x09, false),
    (0x26, 0x0A, false),
    (0x27, 0x0B, false),
    // Controle e pontuação.
    (0x28, 0x1C, false),
    (0x29, 0x01, false),
    (0x2A, 0x0E, false),
    (0x2B, 0x0F, false),
    (0x2C, 0x39, false),
    (0x2D, 0x0C, false),
    (0x2E, 0x0D, false),
    (0x2F, 0x1A, false),
    (0x30, 0x1B, false),
    (0x31, 0x2B, false),
    (0x33, 0x27, false),
    (0x34, 0x28, false),
    (0x35, 0x29, false),
    (0x36, 0x33, false),
    (0x37, 0x34, false),
    (0x38, 0x35, false),
    (0x39, 0x3A, false),
    // Função.
    (0x3A, 0x3B, false),
    (0x3B, 0x3C, false),
    (0x3C, 0x3D, false),
    (0x3D, 0x3E, false),
    (0x3E, 0x3F, false),
    (0x3F, 0x40, false),
    (0x40, 0x41, false),
    (0x41, 0x42, false),
    (0x42, 0x43, false),
    (0x43, 0x44, false),
    (0x44, 0x57, false),
    (0x45, 0x58, false),
    // Navegação (estendidas).
    (0x49, 0x52, true),
    (0x4A, 0x47, true),
    (0x4B, 0x49, true),
    (0x4C, 0x53, true),
    (0x4D, 0x4F, true),
    (0x4E, 0x51, true),
    (0x4F, 0x4D, true),
    (0x50, 0x4B, true),
    (0x51, 0x50, true),
    (0x52, 0x48, true),
    // Modificadores.
    (0xE0, 0x1D, false),
    (0xE1, 0x2A, false),
    (0xE2, 0x38, false),
    (0xE3, 0x5B, true),
    (0xE4, 0x1D, true),
    (0xE5, 0x36, false),
    (0xE6, 0x38, true),
    (0xE7, 0x5C, true),
];

/// O scancode e o bit estendido para um HID Usage, se houver correspondência.
#[must_use]
pub fn hid_to_scancode(usage: HidUsage) -> Option<(u16, bool)> {
    MAP.iter()
        .find(|(hid, _, _)| *hid == usage.get())
        .map(|(_, scancode, extended)| (*scancode, *extended))
}

/// O HID Usage para um scancode do conjunto 1, com o bit estendido do gancho.
#[must_use]
pub fn scancode_to_hid(scancode: u16, extended: bool) -> Option<HidUsage> {
    MAP.iter()
        .find(|(_, code, ext)| *code == scancode && *ext == extended)
        .map(|(hid, _, _)| HidUsage(*hid))
        // Ctrl e Alt esquerdos e o Shift direito chegam sem o bit estendido em alguns teclados;
        // se a busca exata falhar, tenta a variante não estendida.
        .or_else(|| {
            MAP.iter()
                .find(|(_, code, _)| *code == scancode)
                .map(|(hid, _, _)| HidUsage(*hid))
        })
}

/// Todos os scancodes conhecidos, para soltar tudo.
#[must_use]
pub fn all_scancodes() -> Vec<(u16, bool)> {
    MAP.iter().map(|(_, code, ext)| (*code, *ext)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_map_round_trips_for_exact_entries() {
        for (hid, scancode, extended) in MAP {
            assert_eq!(
                hid_to_scancode(HidUsage(*hid)),
                Some((*scancode, *extended))
            );
            assert_eq!(
                scancode_to_hid(*scancode, *extended),
                Some(HidUsage(*hid)),
                "scancode {scancode:#x} ext={extended}"
            );
        }
    }

    #[test]
    fn a_letter_maps_to_its_set1_scancode() {
        // 'a' HID 0x04 → scancode 0x1E no conjunto 1.
        assert_eq!(hid_to_scancode(HidUsage(0x04)), Some((0x1E, false)));
    }

    #[test]
    fn an_unmapped_usage_has_no_scancode() {
        assert_eq!(hid_to_scancode(HidUsage(0xAA)), None);
    }
}
