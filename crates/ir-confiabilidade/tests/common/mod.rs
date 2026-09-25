//! O que os testes de confiabilidade têm em comum: um quadro qualquer com a sequência dada, um
//! instante em milissegundos, e o piso e o teto de retransmissão.

#![allow(dead_code, unreachable_pub)]

use ir_confiabilidade::time::{Millis, Timestamp};
use ir_proto::frame::{Frame, Sequence};
use ir_proto::input::{HidUsage, Modifiers};
use ir_proto::message::{InputMessage, Message};

/// O piso de retransmissão dos testes.
pub const FLOOR: Millis = Millis(20);
/// O prazo de queda dos testes: o 1 s do produto.
pub const CEILING: Millis = Millis(1000);

/// Um quadro de tecla com esta sequência. O conteúdo não importa; a sequência, sim.
pub fn frame(seq: u32) -> Frame {
    Frame::new(
        Message::Input(InputMessage::KeyDown {
            usage: HidUsage(0x04),
            mods: Modifiers::NONE,
        }),
        Sequence(seq),
    )
}

/// O instante `millis` depois do zero.
pub fn at(millis: u64) -> Timestamp {
    Timestamp::from_millis(millis)
}
