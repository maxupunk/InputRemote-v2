//! Injeção por `SendInput`, o caminho do cliente no Windows.
//!
//! Teclado por scancode ([05, §4.1](../../../docs/05-windows.md)) e ponteiro **absoluto** sobre o
//! desktop virtual ([05, §4.2](../../../docs/05-windows.md)), para o Windows não aplicar
//! aceleração a deltas que já vêm acelerados. A posição chega relativa a um monitor e é posta no
//! desktop virtual pelo [`ArranjoLocal`].

#![allow(unsafe_code)]
#![allow(unreachable_pub)]

use ir_proto::input::{Button, HidUsage, WheelDelta};
use ir_proto::screens::ScreenLayout;
use windows::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    INPUT, INPUT_0, INPUT_KEYBOARD, INPUT_MOUSE, KEYBD_EVENT_FLAGS, KEYBDINPUT,
    KEYEVENTF_EXTENDEDKEY, KEYEVENTF_KEYUP, KEYEVENTF_SCANCODE, MOUSE_EVENT_FLAGS,
    MOUSEEVENTF_ABSOLUTE, MOUSEEVENTF_HWHEEL, MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP,
    MOUSEEVENTF_MIDDLEDOWN, MOUSEEVENTF_MIDDLEUP, MOUSEEVENTF_MOVE, MOUSEEVENTF_RIGHTDOWN,
    MOUSEEVENTF_RIGHTUP, MOUSEEVENTF_VIRTUALDESK, MOUSEEVENTF_WHEEL, MOUSEEVENTF_XDOWN,
    MOUSEEVENTF_XUP, MOUSEINPUT, SendInput, VIRTUAL_KEY,
};

use crate::error::{InputError, Result};

/// Os modificadores, nas duas mãos, pelo código virtual: Ctrl, Shift, Alt e as teclas Windows.
const MODIFICADORES: &[u16] = &[0xA0, 0xA1, 0xA2, 0xA3, 0xA4, 0xA5, 0x5B, 0x5C];

/// O Alt esquerdo e o direito, que precisam do disfarce contra a barra de menus.
const ALT: &[u16] = &[0xA4, 0xA5];

/// Um código virtual sem função nenhuma (`VK_NONAME`), que serve de disfarce.
const SEM_FUNCAO: u16 = 0xFC;

/// Solta os modificadores que o **sistema** ainda julga apertados.
///
/// Acontece a cada travessia: o usuário aperta Ctrl aqui, atravessa segurando, e solta do outro
/// lado. O "soltar" é comido pela supressão, e o Windows fica achando que o Ctrl continua
/// apertado — clicar passa a selecionar vários itens. Antes isto era mascarado sem querer, pelo
/// "solta tudo" que soltava o teclado inteiro; agora é explícito, e só mexe nos modificadores,
/// que são os únicos que grudam.
pub(crate) fn soltar_modificadores_presos() {
    let presos = presos(|vk| {
        // SAFETY: a função só lê o estado de uma tecla e não tem pré-condição.
        let estado = unsafe { GetAsyncKeyState(i32::from(vk)) };
        // O bit alto diz "apertada agora"; em `i16` ele é o bit de sinal.
        estado < 0
    });
    let mut inputs = Vec::new();
    for vk in presos {
        // Soltar o Alt sozinho ativa a barra de menus do programa em foco — o mesmo defeito do
        // menu de contexto, por outra porta. Uma tecla sem função entre o apertar e o soltar
        // desfaz isso: para o Windows, o Alt deixou de estar sozinho.
        if ALT.contains(&vk) {
            inputs.push(tecla_virtual(SEM_FUNCAO, true));
            inputs.push(tecla_virtual(SEM_FUNCAO, false));
        }
        inputs.push(tecla_virtual(vk, false));
    }
    if !inputs.is_empty() {
        let _ = send(&inputs);
    }
}

/// Quais modificadores estão apertados, pela pergunta dada. Separado do sistema para ser testado.
fn presos(esta_apertada: impl Fn(u16) -> bool) -> Vec<u16> {
    MODIFICADORES
        .iter()
        .copied()
        .filter(|vk| esta_apertada(*vk))
        .collect()
}

/// Um evento de tecla pelo código virtual, e não por scancode: aqui o alvo é o estado que o
/// **sistema** guarda, não o layout da máquina controlada.
fn tecla_virtual(vk: u16, pressionada: bool) -> INPUT {
    let flags = if pressionada {
        KEYBD_EVENT_FLAGS(0)
    } else {
        KEYEVENTF_KEYUP
    };
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: VIRTUAL_KEY(vk),
                wScan: 0,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

/// Botões laterais, na parte alta de `mouseData`.
const XBUTTON1: u16 = 0x0001;
const XBUTTON2: u16 = 0x0002;
use crate::arranjo::ArranjoLocal;
use crate::pendentes::Pendentes;
use crate::windows::scancode::hid_to_scancode;
use crate::{InjectEvent, Injector};

/// O injetor por `SendInput`. Cada evento é uma chamada.
#[derive(Debug, Default)]
pub struct SendInputInjector {
    /// O que este injetor apertou e ainda não soltou.
    pendentes: Pendentes,
    /// Os monitores, para a posição de um deles virar posição no desktop virtual.
    arranjo: ArranjoLocal,
}

impl SendInputInjector {
    /// Um injetor novo.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            pendentes: Pendentes::nova(),
            arranjo: ArranjoLocal::nenhum(),
        }
    }
}

impl Injector for SendInputInjector {
    fn inject(&mut self, event: InjectEvent) -> Result<()> {
        match event {
            InjectEvent::Key { usage, pressed } => {
                inject_key(usage, pressed)?;
                self.pendentes.tecla(usage, pressed);
                Ok(())
            }
            InjectEvent::Button { button, pressed } => {
                send(&[button_input(button, pressed)])?;
                self.pendentes.botao(button, pressed);
                Ok(())
            }
            InjectEvent::Wheel(delta) => inject_wheel(delta),
            InjectEvent::Pointer(position) => {
                let (x, y) = self.arranjo.no_desktop_virtual(position);
                send(&[pointer_input(x, y)])
            }
        }
    }

    fn usar_telas(&mut self, telas: &ScreenLayout) {
        self.arranjo.usar(telas);
    }

    /// Solta o que **este injetor** apertou — e nada mais.
    ///
    /// Soltar o que não está preso não é inócuo no Windows: um botão direito solto abre o menu de
    /// contexto do programa em foco, e um Alt solto ativa a barra de menus. Era o que acontecia a
    /// cada volta do ponteiro ao servidor ([log 40](../../../docs/logs/40-o-ajudante-que-ninguem-subia.md)).
    fn release_all(&mut self) -> Result<()> {
        let (teclas, botoes) = self.pendentes.soltar();
        let mut inputs = Vec::new();
        for usage in teclas {
            if let Some((scancode, extended)) = hid_to_scancode(usage) {
                inputs.push(key_input(scancode, extended, false));
            }
        }
        for button in botoes {
            inputs.push(button_input(button, false));
        }
        send(&inputs)
    }
}

/// Envia um lote de eventos, tratando a recusa do sistema.
fn send(inputs: &[INPUT]) -> Result<()> {
    if inputs.is_empty() {
        return Ok(());
    }
    let size = i32::try_from(core::mem::size_of::<INPUT>()).unwrap_or(0);
    // SAFETY: `inputs` é uma fatia de `INPUT` bem formados e `size` é o tamanho do tipo. Um valor
    // de retorno menor que o esperado significa que o sistema recusou parte dos eventos — o
    // sintoma do endurecimento de janeiro de 2026 ([05, §4.4](../../../docs/05-windows.md)).
    let sent = unsafe { SendInput(inputs, size) };
    if sent as usize == inputs.len() {
        Ok(())
    } else {
        Err(InputError::Rejected)
    }
}

fn inject_key(usage: HidUsage, pressed: bool) -> Result<()> {
    let Some((scancode, extended)) = hid_to_scancode(usage) else {
        return Err(InputError::UnmappedKey);
    };
    send(&[key_input(scancode, extended, pressed)])
}

fn key_input(scancode: u16, extended: bool, pressed: bool) -> INPUT {
    let mut flags = KEYEVENTF_SCANCODE;
    if extended {
        flags |= KEYEVENTF_EXTENDEDKEY;
    }
    if !pressed {
        flags |= KEYEVENTF_KEYUP;
    }
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: VIRTUAL_KEY(0),
                wScan: scancode,
                dwFlags: KEYBD_EVENT_FLAGS(flags.0),
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

fn button_input(button: Button, pressed: bool) -> INPUT {
    let (flags, data) = match (button, pressed) {
        (Button::Left, true) => (MOUSEEVENTF_LEFTDOWN, 0),
        (Button::Left, false) => (MOUSEEVENTF_LEFTUP, 0),
        (Button::Right, true) => (MOUSEEVENTF_RIGHTDOWN, 0),
        (Button::Right, false) => (MOUSEEVENTF_RIGHTUP, 0),
        (Button::Middle, true) => (MOUSEEVENTF_MIDDLEDOWN, 0),
        (Button::Middle, false) => (MOUSEEVENTF_MIDDLEUP, 0),
        (Button::Back, true) => (MOUSEEVENTF_XDOWN, i32::from(XBUTTON1)),
        (Button::Back, false) => (MOUSEEVENTF_XUP, i32::from(XBUTTON1)),
        (Button::Forward, true) => (MOUSEEVENTF_XDOWN, i32::from(XBUTTON2)),
        (Button::Forward, false) => (MOUSEEVENTF_XUP, i32::from(XBUTTON2)),
    };
    mouse_input(flags, 0, 0, data)
}

fn inject_wheel(delta: WheelDelta) -> Result<()> {
    let mut inputs = Vec::new();
    if delta.dy != 0 {
        inputs.push(mouse_input(MOUSEEVENTF_WHEEL, 0, 0, i32::from(delta.dy)));
    }
    if delta.dx != 0 {
        inputs.push(mouse_input(MOUSEEVENTF_HWHEEL, 0, 0, i32::from(delta.dx)));
    }
    send(&inputs)
}

/// O ponteiro absoluto sobre o desktop virtual inteiro, em `0..=65535` nos dois eixos — o
/// referencial de `MOUSEEVENTF_VIRTUALDESK`, e não o de um monitor.
fn pointer_input(x: u16, y: u16) -> INPUT {
    mouse_input(
        MOUSEEVENTF_MOVE | MOUSEEVENTF_ABSOLUTE | MOUSEEVENTF_VIRTUALDESK,
        i32::from(x),
        i32::from(y),
        0,
    )
}

fn mouse_input(flags: MOUSE_EVENT_FLAGS, dx: i32, dy: i32, data: i32) -> INPUT {
    INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 {
            mi: MOUSEINPUT {
                dx,
                dy,
                #[allow(clippy::cast_sign_loss)]
                mouseData: data as u32,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn so_os_modificadores_apertados_entram_na_lista() {
        // O defeito: o Ctrl apertado aqui e solto do outro lado ficava preso, porque a supressão
        // comeu o "soltar". Clicar no Explorer passava a selecionar vários itens.
        let apertados = [0xA2_u16, 0xA4];
        let lista = presos(|vk| apertados.contains(&vk));
        assert_eq!(lista, vec![0xA2, 0xA4]);
        assert!(presos(|_| false).is_empty(), "nada apertado, nada a soltar");
    }

    #[test]
    fn o_alt_preso_vai_disfarcado_para_nao_abrir_a_barra_de_menus() {
        // Três eventos para o Alt: a tecla sem função (apertar e soltar) e então o Alt solto.
        let so_alt = presos(|vk| vk == 0xA4);
        assert_eq!(so_alt, vec![0xA4]);
        assert!(ALT.contains(&0xA4) && ALT.contains(&0xA5));
        assert!(!ALT.contains(&0xA2), "o Ctrl não precisa de disfarce");
    }
}
