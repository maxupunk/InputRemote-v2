//! A tabela de vetores gravados, um grupo por canal.
//!
//! Separada do arquivo de testes porque é dado, não lógica — e porque o limite de 400 linhas
//! por arquivo de `docs/09-padroes-de-codigo.md` §1 vale também para tabela.

// Este módulo só é usado pelo binário de teste ao lado; `pub` aqui é a única forma de
// exportá-lo para lá.
#![allow(unreachable_pub)]

use ir_proto::frame::{Ack, Frame, Sequence};
use ir_proto::ids::{MachineId, MonitorId};
use ir_proto::input::{
    Button, HidUsage, InputState, Modifiers, PointerDelta, PointerPosition, WheelDelta,
};
use ir_proto::message::{Control, Feedback, Greeting, InputMessage, Message, PointerMessage};
use ir_proto::peer::{Capabilities, ClipboardCapabilities, MachineName, PrivilegedInputLevel};
use ir_proto::screens::{Edge, ScreenLayout};
use ir_proto::version;

/// Um vetor: nome, o quadro, e os bytes que a versão 1 produz para ele.
pub struct Vector {
    pub name: &'static str,
    pub frame: Frame,
    pub hex: &'static str,
}

/// Modificadores usados em todos os vetores de entrada: um da esquerda, um da direita.
fn mods() -> Modifiers {
    Modifiers::LEFT_CTRL.union(Modifiers::RIGHT_SHIFT)
}

/// Posição usada em todos os vetores que precisam de uma.
fn position() -> PointerPosition {
    PointerPosition {
        monitor: MonitorId(1),
        x: 0x1234,
        y: 0xABCD,
    }
}

/// Estado com uma tecla comum, um modificador e um botão pressionados.
fn state() -> InputState {
    let mut state = InputState::released();
    state.apply_key(HidUsage::LEFT_CTRL, true);
    state.apply_key(HidUsage(0x04), true);
    state.apply_button(Button::Left, true);
    state
}

fn greeting() -> Greeting {
    Greeting {
        version: version::CURRENT,
        machine: MachineId([
            0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0x0c, 0x0d, 0x0e,
            0x0f, 0x10,
        ]),
        name: MachineName::new("bancada").expect("nome válido"),
        capabilities: Capabilities {
            clipboard: ClipboardCapabilities::ALL,
            bulk_transfer: true,
            privileged_input: PrivilegedInputLevel::LockScreen,
            secure_attention: false,
        },
    }
}

/// Construtores curtos, compartilhados pelos quatro grupos de vetores.
fn v(name: &'static str, frame: Frame, hex: &'static str) -> Vector {
    Vector { name, frame, hex }
}

fn control(message: Control, seq: u32) -> Frame {
    Frame::new(Message::Control(message), Sequence(seq))
}

fn input(message: InputMessage, seq: u32) -> Frame {
    Frame::new(Message::Input(message), Sequence(seq))
}

fn pointer(message: PointerMessage, seq: u32) -> Frame {
    Frame::new(Message::Pointer(message), Sequence(seq))
}

fn feedback(message: Feedback, seq: u32) -> Frame {
    Frame::new(Message::Feedback(message), Sequence(seq))
}

/// Todos os vetores gravados, um grupo por canal.
///
/// Dividido por canal e não numa tabela só porque o limite de 60 linhas por função de
/// `docs/09-padroes-de-codigo.md` §1 vale também para tabela de dados — e porque um grupo
/// por canal é onde se procura quando um canal muda.
pub fn vectors() -> Vec<Vector> {
    let mut all = control_vectors();
    all.extend(session_vectors());
    all.extend(input_vectors());
    all.extend(pointer_vectors());
    all.extend(feedback_vectors());
    all
}

/// Canal 0 — handshake e configuração.
fn control_vectors() -> Vec<Vector> {
    vec![
        v(
            "hello",
            control(Control::Hello(greeting()), 1),
            "0000010102030405060708090a0b0c0d0e0f100762616e636164610101010102000100",
        ),
        v(
            "screens",
            control(
                Control::Screens(ScreenLayout::single(1920, 1080).expect("arranjo válido")),
                2,
            ),
            "000201000000800fb808e807010200",
        ),
        v(
            "edge_config",
            control(
                Control::EdgeConfig {
                    peer_edge: Edge::Right,
                },
                3,
            ),
            "0003010300",
        ),
    ]
}

/// Canal 0 — travessia, estado e heartbeat.
fn session_vectors() -> Vec<Vector> {
    vec![
        v(
            "enter_screen",
            control(
                Control::EnterScreen {
                    entering_edge: Edge::Left,
                    position: position(),
                    state: state(),
                },
                4,
            ),
            "00040001b424cdd7020204e00101010400",
        ),
        v(
            "state_snapshot",
            control(
                Control::StateSnapshot {
                    state: state(),
                    position: position(),
                },
                5,
            ),
            "00060204e001010101b424cdd7020500",
        ),
        v(
            "ping",
            control(
                Control::Ping {
                    stamp_micros: 0x0011_2233_4455_6677,
                },
                6,
            ),
            "0007f7ccd5a2b4c6c8080600",
        ),
        v(
            "ack_only",
            control(Control::AckOnly, 7).with_ack(Ack {
                cumulative: Sequence(99),
                bits: 0x0000_00ff,
            }),
            "0009070163ff01",
        ),
    ]
}

/// Canal 1 — entrada confiável.
fn input_vectors() -> Vec<Vector> {
    vec![
        v(
            "key_down",
            input(
                InputMessage::KeyDown {
                    usage: HidUsage(0x04),
                    mods: mods(),
                },
                8,
            ),
            "010004210800",
        ),
        v(
            "key_up",
            input(
                InputMessage::KeyUp {
                    usage: HidUsage::RIGHT_GUI,
                    mods: mods(),
                },
                9,
            ),
            "0101e701210900",
        ),
        v(
            "button_down",
            input(
                InputMessage::ButtonDown {
                    button: Button::Forward,
                    mods: mods(),
                },
                10,
            ),
            "010204210a00",
        ),
        v(
            "wheel",
            input(
                InputMessage::Wheel {
                    delta: WheelDelta::down(),
                    mods: mods(),
                },
                11,
            ),
            "010400ef01210b00",
        ),
        v(
            "release_all",
            input(InputMessage::ReleaseAll, 12),
            "01050c00",
        ),
    ]
}

/// Canal 2 — ponteiro.
fn pointer_vectors() -> Vec<Vector> {
    vec![
        v(
            "pointer_motion",
            pointer(
                PointerMessage::Motion {
                    delta: PointerDelta { dx: -7, dy: 300 },
                    mods: mods(),
                },
                13,
            ),
            "02000dd804210d00",
        ),
        v(
            "pointer_position",
            pointer(
                PointerMessage::Position {
                    position: position(),
                    mods: mods(),
                },
                14,
            ),
            "020101b424cdd702210e00",
        ),
    ]
}

/// Canal 3 — retorno.
fn feedback_vectors() -> Vec<Vector> {
    vec![
        v(
            "edge_reached",
            feedback(
                Feedback::EdgeReached {
                    edge: Edge::Bottom,
                    position: position(),
                },
                15,
            ),
            "03000301b424cdd7020f00",
        ),
        v(
            "emergency",
            feedback(Feedback::EmergencyRelease, 16),
            "03011000",
        ),
    ]
}
