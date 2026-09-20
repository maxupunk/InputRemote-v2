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
/// Cobre o teclado inteiro de 104 teclas, menos o Pause — as mesmas teclas que o Linux injeta
/// (`linux::keymap`). Uma tecla que falte aqui é uma tecla que **some** na travessia, sem erro
/// nenhum: foi o que aconteceu com o `PrintScreen` e com o teclado numérico.
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
    // Impressão de tela e Scroll Lock. Faltavam: a tecla era capturada pelo gancho, não
    // encontrava HID nenhum e sumia ali — apertar PrintScreen no servidor não fazia nada no
    // cliente. O `0x37` sem o bit estendido é o asterisco do teclado numérico, logo abaixo; é o
    // bit que separa os dois. Pause fica de fora de propósito: no conjunto 1 ele é a sequência
    // `E1 1D 45`, e o `0x45` cru é o Num Lock.
    (0x46, 0x37, true),
    // O mesmo PrintScreen com Alt segurado (a captura só da janela ativa): o teclado manda `0x54`,
    // o SysRq, sem o bit estendido. É a segunda linha do mesmo HID; a injeção usa a primeira.
    (0x46, 0x54, false),
    (0x47, 0x46, false),
    // Teclado numérico. Os dígitos e o ponto compartilham scancode com o bloco de navegação
    // (`Home`, `End`, as setas), que está logo abaixo com o bit estendido.
    (0x53, 0x45, false),
    (0x54, 0x35, true),
    (0x55, 0x37, false),
    (0x56, 0x4A, false),
    (0x57, 0x4E, false),
    (0x58, 0x1C, true),
    (0x59, 0x4F, false),
    (0x5A, 0x50, false),
    (0x5B, 0x51, false),
    (0x5C, 0x4B, false),
    (0x5D, 0x4C, false),
    (0x5E, 0x4D, false),
    (0x5F, 0x47, false),
    (0x60, 0x48, false),
    (0x61, 0x49, false),
    (0x62, 0x52, false),
    (0x63, 0x53, false),
    // A tecla de menu de contexto, entre o Windows direito e o Ctrl direito.
    (0x65, 0x5D, true),
    // As teclas que o teclado americano não tem, e o ABNT2 (o brasileiro) tem: a `\ |` à esquerda
    // do Z, a `/ ? °` à esquerda do Shift direito, e a vírgula do teclado numérico. Scancodes
    // conferidos no Windows por `VK_OEM_102`, `VK_ABNT_C1` e `VK_ABNT_C2`.
    (0x64, 0x56, false),
    (0x85, 0x7E, false),
    (0x87, 0x73, false),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_map_round_trips_for_exact_entries() {
        for (hid, scancode, extended) in MAP {
            // Um HID pode ter mais de um scancode (o PrintScreen com e sem Alt); a injeção usa o
            // primeiro, e a captura reconhece todos.
            let primeiro = MAP
                .iter()
                .find(|(outro, _, _)| outro == hid)
                .map(|(_, code, ext)| (*code, *ext));
            assert_eq!(hid_to_scancode(HidUsage(*hid)), primeiro);
            assert_eq!(
                scancode_to_hid(*scancode, *extended),
                Some(HidUsage(*hid)),
                "scancode {scancode:#x} ext={extended}"
            );
        }
    }

    #[test]
    fn o_printscreen_com_alt_e_o_mesmo_hid() {
        // Medido no Windows: `MapVirtualKeyW(VK_SNAPSHOT, MAPVK_VK_TO_VSC_EX)` devolve `0x54`, que
        // é o que o teclado manda com Alt segurado; sozinho, ele manda `E0 37`.
        assert_eq!(scancode_to_hid(0x54, false), Some(HidUsage(0x46)));
        assert_eq!(hid_to_scancode(HidUsage(0x46)), Some((0x37, true)));
    }

    #[test]
    fn a_letter_maps_to_its_set1_scancode() {
        // 'a' HID 0x04 → scancode 0x1E no conjunto 1.
        assert_eq!(hid_to_scancode(HidUsage(0x04)), Some((0x1E, false)));
    }

    #[test]
    fn printscreen_e_o_teclado_numerico_atravessam() {
        // O defeito: PrintScreen no servidor não fazia nada no cliente, porque o gancho recebia o
        // scancode e não achava HID nenhum. O bit estendido é o que o separa do asterisco do
        // teclado numérico, que tem o mesmo scancode.
        assert_eq!(scancode_to_hid(0x37, true), Some(HidUsage(0x46)));
        assert_eq!(scancode_to_hid(0x37, false), Some(HidUsage(0x55)));
        assert_eq!(hid_to_scancode(HidUsage(0x46)), Some((0x37, true)));

        // Os dígitos do teclado numérico contra o bloco de navegação, mesmo scancode.
        assert_eq!(scancode_to_hid(0x4F, false), Some(HidUsage(0x59)), "KP1");
        assert_eq!(scancode_to_hid(0x4F, true), Some(HidUsage(0x4D)), "End");
        assert_eq!(
            scancode_to_hid(0x1C, true),
            Some(HidUsage(0x58)),
            "Enter do KP"
        );
    }

    /// Uma tecla que o Linux injeta e o Windows não sabe capturar some na travessia, calada. A
    /// única diferença aceita é o Pause, que no conjunto 1 é uma sequência de três scancodes.
    #[test]
    fn o_windows_conhece_as_mesmas_teclas_que_o_linux() {
        const PAUSE: u16 = 0x48;
        let mapeados: Vec<u16> = MAP.iter().map(|(hid, _, _)| *hid).collect();
        for usage in ir_proto::input::teclado_completo() {
            let conhecido = mapeados.contains(&usage.get()) || usage.get() == PAUSE;
            assert!(
                conhecido,
                "HID {:#04x} sem scancode no Windows",
                usage.get()
            );
        }
    }

    #[test]
    fn an_unmapped_usage_has_no_scancode() {
        assert_eq!(hid_to_scancode(HidUsage(0xAA)), None);
    }
}
