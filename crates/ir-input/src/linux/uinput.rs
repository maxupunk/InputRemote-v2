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
use ir_proto::screens::ScreenLayout;

use crate::arranjo::ArranjoLocal;
use crate::error::{InputError, Result};
use crate::linux::keymap::{all_keys, button_to_key, hid_to_key};
use crate::linux::nome_virtual;
use crate::pendentes::Pendentes;
use crate::roda::AcumuladorDeRoda;
use crate::{InjectEvent, Injector};

/// O maior valor absoluto de um eixo do ponteiro. `0..=65535` cobre o desktop virtual inteiro,
/// e o compositor mapeia essa faixa para a tela — por isso a posição independe da resolução.
const ABS_MAX: i32 = 65_535;

/// O injetor `uinput`, dono dos dispositivos virtuais.
pub struct UinputInjector {
    keyboard: VirtualDevice,
    pointer: VirtualDevice,
    /// O que este injetor apertou e ainda não soltou.
    pendentes: Pendentes,
    /// Os monitores, para a posição de um deles virar posição no desktop virtual.
    arranjo: ArranjoLocal,
    /// A roda fina que ainda não completou uma marcação — `REL_WHEEL` só anda marcações inteiras.
    roda: AcumuladorDeRoda,
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
        Ok(Self {
            keyboard,
            pointer,
            pendentes: Pendentes::nova(),
            arranjo: ArranjoLocal::nenhum(),
            roda: AcumuladorDeRoda::novo(f32::from(WheelDelta::NOTCH)),
        })
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
        let Some(key) = button_to_key(button) else {
            return Err(InputError::Unsupported);
        };
        let value = i32::from(pressed);
        self.pointer
            .emit(&[InputEvent::new(EventType::KEY, key.code(), value)])
            .map_err(|e| InputError::Io(e.to_string()))
    }

    fn wheel_event(&mut self, delta: WheelDelta) -> Result<()> {
        let mut events = Vec::new();
        // A roda fina do touchpad de precisão chega abaixo de uma marcação; o resto fica guardado
        // até completar uma, em vez de sumir na divisão.
        let (notches_h, notches_v) = self.roda.acumular(f32::from(delta.dx), f32::from(delta.dy));
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
        // A posição chega relativa a um monitor; o eixo absoluto cobre o desktop virtual inteiro.
        let (x, y) = self.arranjo.no_desktop_virtual(position);
        self.pointer
            .emit(&[
                InputEvent::new(EventType::ABSOLUTE, AbsoluteAxisType::ABS_X.0, i32::from(x)),
                InputEvent::new(EventType::ABSOLUTE, AbsoluteAxisType::ABS_Y.0, i32::from(y)),
            ])
            .map_err(|e| InputError::Io(e.to_string()))
    }
}

impl Injector for UinputInjector {
    fn inject(&mut self, event: InjectEvent) -> Result<()> {
        match event {
            InjectEvent::Key { usage, pressed } => {
                self.key_event(usage, pressed)?;
                self.pendentes.tecla(usage, pressed);
                Ok(())
            }
            InjectEvent::Button { button, pressed } => {
                self.button_event(button, pressed)?;
                self.pendentes.botao(button, pressed);
                Ok(())
            }
            InjectEvent::Wheel(delta) => self.wheel_event(delta),
            InjectEvent::Pointer(position) => self.pointer_event(position),
        }
    }

    fn usar_telas(&mut self, telas: &ScreenLayout) {
        self.arranjo.usar(telas);
    }

    /// Solta o que **este injetor** apertou — e nada mais.
    ///
    /// Soltava toda tecla e todo botão que o dispositivo sabe emitir. Aqui isso não abre menu como
    /// no Windows, mas é a mesma mentira: o produto dizia ao sistema que soltou o que nunca
    /// apertou. Continua idempotente, que era a razão de soltar tudo.
    fn release_all(&mut self) -> Result<()> {
        let (teclas, botoes) = self.pendentes.soltar();
        let events: Vec<InputEvent> = teclas
            .into_iter()
            .filter_map(hid_to_key)
            .map(|key| InputEvent::new(EventType::KEY, key.code(), 0))
            .collect();
        if !events.is_empty() {
            self.keyboard
                .emit(&events)
                .map_err(|e| InputError::Io(e.to_string()))?;
        }

        let buttons: Vec<InputEvent> = botoes
            .into_iter()
            .filter_map(button_to_key)
            .map(|key| InputEvent::new(EventType::KEY, key.code(), 0))
            .collect();
        if buttons.is_empty() {
            return Ok(());
        }
        self.pointer
            .emit(&buttons)
            .map_err(|e| InputError::Io(e.to_string()))
    }
}

fn build_keyboard() -> Result<VirtualDevice> {
    let mut keys = AttributeSet::<Key>::new();
    for key in all_keys() {
        keys.insert(key);
    }
    VirtualDeviceBuilder::new()
        .map_err(|e| InputError::Device(e.to_string()))?
        .name(&nome_virtual("Keyboard"))
        .with_keys(&keys)
        .map_err(|e| InputError::Device(e.to_string()))?
        .build()
        .map_err(|e| InputError::Device(e.to_string()))
}

fn build_pointer() -> Result<VirtualDevice> {
    let mut buttons = AttributeSet::<Key>::new();
    for key in Button::ALL.into_iter().filter_map(button_to_key) {
        buttons.insert(key);
    }
    let mut rel = AttributeSet::<RelativeAxisType>::new();
    rel.insert(RelativeAxisType::REL_WHEEL);
    rel.insert(RelativeAxisType::REL_HWHEEL);

    let abs = AbsInfo::new(0, 0, ABS_MAX, 0, 0, 1);
    let abs_x = UinputAbsSetup::new(AbsoluteAxisType::ABS_X, abs);
    let abs_y = UinputAbsSetup::new(AbsoluteAxisType::ABS_Y, abs);

    VirtualDeviceBuilder::new()
        .map_err(|e| InputError::Device(e.to_string()))?
        .name(&nome_virtual("Pointer"))
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
