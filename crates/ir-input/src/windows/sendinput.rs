//! Injeção por `SendInput`, o caminho do cliente no Windows.
//!
//! Teclado por scancode ([05, §4.1](../../../docs/05-windows.md)) e ponteiro **absoluto** sobre o
//! desktop virtual ([05, §4.2](../../../docs/05-windows.md)), para o Windows não aplicar
//! aceleração a deltas que já vêm acelerados.

#![allow(unsafe_code)]
#![allow(unreachable_pub)]

use ir_proto::input::{Button, HidUsage, PointerPosition, WheelDelta};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    INPUT, INPUT_0, INPUT_KEYBOARD, INPUT_MOUSE, KEYBD_EVENT_FLAGS, KEYBDINPUT,
    KEYEVENTF_EXTENDEDKEY, KEYEVENTF_KEYUP, KEYEVENTF_SCANCODE, MOUSE_EVENT_FLAGS,
    MOUSEEVENTF_ABSOLUTE, MOUSEEVENTF_HWHEEL, MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP,
    MOUSEEVENTF_MIDDLEDOWN, MOUSEEVENTF_MIDDLEUP, MOUSEEVENTF_MOVE, MOUSEEVENTF_RIGHTDOWN,
    MOUSEEVENTF_RIGHTUP, MOUSEEVENTF_VIRTUALDESK, MOUSEEVENTF_WHEEL, MOUSEEVENTF_XDOWN,
    MOUSEEVENTF_XUP, MOUSEINPUT, SendInput, VIRTUAL_KEY,
};

use crate::error::{InputError, Result};

/// Botões laterais, na parte alta de `mouseData`.
const XBUTTON1: u16 = 0x0001;
const XBUTTON2: u16 = 0x0002;
use crate::windows::scancode::{all_scancodes, hid_to_scancode};
use crate::{InjectEvent, Injector};

/// O injetor por `SendInput`. Sem estado próprio: cada evento é uma chamada.
#[derive(Debug, Default)]
pub struct SendInputInjector;

impl SendInputInjector {
    /// Um injetor novo.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

impl Injector for SendInputInjector {
    fn inject(&mut self, event: InjectEvent) -> Result<()> {
        match event {
            InjectEvent::Key { usage, pressed } => inject_key(usage, pressed),
            InjectEvent::Button { button, pressed } => send(&[button_input(button, pressed)]),
            InjectEvent::Wheel(delta) => inject_wheel(delta),
            InjectEvent::Pointer(position) => send(&[pointer_input(position)]),
        }
    }

    fn release_all(&mut self) -> Result<()> {
        let mut inputs = Vec::new();
        for (scancode, extended) in all_scancodes() {
            inputs.push(key_input(scancode, extended, false));
        }
        for button in Button::ALL {
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

fn pointer_input(position: PointerPosition) -> INPUT {
    // Absoluto sobre o desktop virtual, em `0..=65535`. Numa tela só, a posição normalizada da
    // mensagem já é a posição absoluta.
    mouse_input(
        MOUSEEVENTF_MOVE | MOUSEEVENTF_ABSOLUTE | MOUSEEVENTF_VIRTUALDESK,
        i32::from(position.x),
        i32::from(position.y),
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
