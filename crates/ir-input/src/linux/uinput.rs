//! Injeção por `uinput`, o caminho do cliente no Linux.
//!
//! `uinput` entra abaixo do compositor, no nível do kernel: funciona no greeter, na tela de
//! bloqueio, em prompts do `polkit`, em qualquer compositor Wayland, no X11 e no console
//! ([06, §2.3](../../../docs/06-linux.md)).
//!
//! Dois dispositivos, criados **na subida** e mantidos pelo tempo de vida do processo — nunca no
//! momento de usar, porque eventos enviados logo após `UI_DEV_CREATE` se perdem em silêncio
//! enquanto o `udev` processa o dispositivo ([06, §2.2](../../../docs/06-linux.md)). Como os
//! dispositivos nascem aqui e a primeira injeção só acontece segundos depois (após o pareamento),
//! essa janela é naturalmente respeitada.
//!
//! O ponteiro é **absoluto** (`ABS_X`/`ABS_Y` em `0..=65535`): injetar movimento relativo faria o
//! compositor aplicar a própria aceleração a deltas que já vêm acelerados do servidor
//! ([06, §2.1](../../../docs/06-linux.md)).

#![allow(unsafe_code)]
#![allow(unreachable_pub)]

use evdev::uinput::{VirtualDevice, VirtualDeviceBuilder};
use evdev::{
    AbsInfo, AbsoluteAxisType, AttributeSet, EventType, InputEvent, Key, RelativeAxisType,
    UinputAbsSetup,
};
use ir_proto::input::{Button, HidUsage, PointerPosition, WheelDelta};

use crate::error::{InputError, Result};
use crate::linux::keymap::{all_keys, hid_to_key};
use crate::{InjectEvent, Injector};

/// O maior valor absoluto de um eixo do ponteiro. `0..=65535` cobre o desktop virtual inteiro,
/// e o compositor mapeia essa faixa para a tela — por isso a posição independe da resolução.
const ABS_MAX: i32 = 65_535;

/// O injetor `uinput`, dono dos dispositivos virtuais.
pub struct UinputInjector {
    keyboard: VirtualDevice,
    pointer: VirtualDevice,
}

impl core::fmt::Debug for UinputInjector {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("UinputInjector")
    }
}

impl UinputInjector {
    /// Cria os dispositivos virtuais.
    ///
    /// # Errors
    ///
    /// [`InputError::Device`] se `/dev/uinput` não puder ser aberto (acesso negado, módulo não
    /// carregado) ou se a criação do dispositivo falhar.
    pub fn open() -> Result<Self> {
        let keyboard = build_keyboard()?;
        let pointer = build_pointer()?;
        Ok(Self { keyboard, pointer })
    }

    fn key_event(&mut self, usage: HidUsage, pressed: bool) -> Result<()> {
        let Some(key) = hid_to_key(usage) else {
            return Err(InputError::UnmappedKey);
        };
        let value = i32::from(pressed);
        self.keyboard
            .emit(&[InputEvent::new(EventType::KEY, key.code(), value)])
            .map_err(|e| InputError::Io(e.to_string()))
    }

    fn button_event(&mut self, button: Button, pressed: bool) -> Result<()> {
        let key = button_key(button);
        let value = i32::from(pressed);
        self.pointer
            .emit(&[InputEvent::new(EventType::KEY, key.code(), value)])
            .map_err(|e| InputError::Io(e.to_string()))
    }

    fn wheel_event(&mut self, delta: WheelDelta) -> Result<()> {
        let mut events = Vec::new();
        let notches_v = i32::from(delta.dy) / i32::from(WheelDelta::NOTCH);
        let notches_h = i32::from(delta.dx) / i32::from(WheelDelta::NOTCH);
        if notches_v != 0 {
            events.push(InputEvent::new(
                EventType::RELATIVE,
                RelativeAxisType::REL_WHEEL.0,
                notches_v,
            ));
        }
        if notches_h != 0 {
            events.push(InputEvent::new(
                EventType::RELATIVE,
                RelativeAxisType::REL_HWHEEL.0,
                notches_h,
            ));
        }
        if events.is_empty() {
            return Ok(());
        }
        self.pointer
            .emit(&events)
            .map_err(|e| InputError::Io(e.to_string()))
    }

    fn pointer_event(&mut self, position: PointerPosition) -> Result<()> {
        // Para uma tela só, a posição normalizada da mensagem (`0..=65535` sobre o monitor) já é
        // a posição absoluta sobre a tela. O caso multi-monitor é um refinamento posterior.
        self.pointer
            .emit(&[
                InputEvent::new(
                    EventType::ABSOLUTE,
                    AbsoluteAxisType::ABS_X.0,
                    i32::from(position.x),
                ),
                InputEvent::new(
                    EventType::ABSOLUTE,
                    AbsoluteAxisType::ABS_Y.0,
                    i32::from(position.y),
                ),
            ])
            .map_err(|e| InputError::Io(e.to_string()))
    }
}

impl Injector for UinputInjector {
    fn inject(&mut self, event: InjectEvent) -> Result<()> {
        match event {
            InjectEvent::Key { usage, pressed } => self.key_event(usage, pressed),
            InjectEvent::Button { button, pressed } => self.button_event(button, pressed),
            InjectEvent::Wheel(delta) => self.wheel_event(delta),
            InjectEvent::Pointer(position) => self.pointer_event(position),
        }
    }

    fn release_all(&mut self) -> Result<()> {
        // Solta toda tecla e todo botão que o dispositivo saiba emitir. É idempotente: soltar o
        // que já está solto não tem efeito, e é a rede de segurança contra tecla presa.
        let mut events = Vec::new();
        for key in all_keys() {
            events.push(InputEvent::new(EventType::KEY, key.code(), 0));
        }
        self.keyboard
            .emit(&events)
            .map_err(|e| InputError::Io(e.to_string()))?;

        let buttons: Vec<InputEvent> = Button::ALL
            .iter()
            .map(|b| InputEvent::new(EventType::KEY, button_key(*b).code(), 0))
            .collect();
        self.pointer
            .emit(&buttons)
            .map_err(|e| InputError::Io(e.to_string()))
    }
}

/// O botão do Linux para um botão do ponteiro do protocolo.
fn button_key(button: Button) -> Key {
    match button {
        Button::Left => Key::BTN_LEFT,
        Button::Right => Key::BTN_RIGHT,
        Button::Middle => Key::BTN_MIDDLE,
        Button::Back => Key::BTN_SIDE,
        Button::Forward => Key::BTN_EXTRA,
    }
}

fn build_keyboard() -> Result<VirtualDevice> {
    let mut keys = AttributeSet::<Key>::new();
    for key in all_keys() {
        keys.insert(key);
    }
    VirtualDeviceBuilder::new()
        .map_err(|e| InputError::Device(e.to_string()))?
        .name("InputRemote Keyboard")
        .with_keys(&keys)
        .map_err(|e| InputError::Device(e.to_string()))?
        .build()
        .map_err(|e| InputError::Device(e.to_string()))
}

fn build_pointer() -> Result<VirtualDevice> {
    let mut buttons = AttributeSet::<Key>::new();
    for button in Button::ALL {
        buttons.insert(button_key(button));
    }
    let mut rel = AttributeSet::<RelativeAxisType>::new();
    rel.insert(RelativeAxisType::REL_WHEEL);
    rel.insert(RelativeAxisType::REL_HWHEEL);

    let abs = AbsInfo::new(0, 0, ABS_MAX, 0, 0, 1);
    let abs_x = UinputAbsSetup::new(AbsoluteAxisType::ABS_X, abs);
    let abs_y = UinputAbsSetup::new(AbsoluteAxisType::ABS_Y, abs);

    VirtualDeviceBuilder::new()
        .map_err(|e| InputError::Device(e.to_string()))?
        .name("InputRemote Pointer")
        .with_keys(&buttons)
        .map_err(|e| InputError::Device(e.to_string()))?
        .with_relative_axes(&rel)
        .map_err(|e| InputError::Device(e.to_string()))?
        .with_absolute_axis(&abs_x)
        .map_err(|e| InputError::Device(e.to_string()))?
        .with_absolute_axis(&abs_y)
        .map_err(|e| InputError::Device(e.to_string()))?
        .build()
        .map_err(|e| InputError::Device(e.to_string()))
}
