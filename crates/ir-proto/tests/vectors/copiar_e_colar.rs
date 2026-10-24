//! Vetores gravados de copiar e colar entre os dois computadores — versões 9 e 10.
//!
//! Em arquivo próprio pelo limite de 400 linhas de `table.rs`: a escolha "copiar e colar" valendo
//! para os dois (`Control::CopyPaste`, versão 9) e o pedido de quem recebe para a cópia parar
//! (`Control::CancelCopy`, versão 10).

#![allow(unreachable_pub)]

use ir_proto::message::Control;
use ir_proto::version::ProtocolVersion;

use crate::table::{Vector, control, greeting, v};

/// Canal 0 — o que entrou depois da versão 8.
pub fn control_vectors_v9() -> Vec<Vector> {
    vec![
        v(
            "hello_v9",
            // Versão 9: copiar e colar valendo para os dois. A saudação só muda no número.
            control(Control::Hello(greeting(ProtocolVersion(9))), 27),
            "0000090102030405060708090a0b0c0d0e0f100762616e63616461010101010200001b0000",
        ),
        v(
            "copy_paste",
            control(
                Control::CopyPaste {
                    enabled: false,
                    chosen_at: 1_759_500_000_000,
                },
                28,
            ),
            "00130080b6fdd29a331c0000",
        ),
        v(
            "hello_v10",
            // Versão 10: quem recebe pede que a cópia pare. A saudação só muda no número.
            control(Control::Hello(greeting(ProtocolVersion(10))), 29),
            "00000a0102030405060708090a0b0c0d0e0f100762616e63616461010101010200001d0000",
        ),
        v(
            "cancel_copy",
            control(Control::CancelCopy, 30),
            "00141e0000",
        ),
    ]
}
