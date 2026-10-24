//! Vetores gravados do recado do painel entre os computadores — versão 11.
//!
//! Em arquivo próprio pelo limite de 400 linhas de `table.rs`. O recado (`Control::Panel`) é opaco
//! aqui; o que vai dentro tem os vetores dele em `ir-monitor/tests/vetores.rs`.
//!
//! Exceção de pré-lançamento (`main.rs`): a versão 11 nasceu com `WatchUsage` e `Usage`, e virou o
//! recado opaco antes de sair desta branch ([log 70](../../../../docs/logs/70-processos-e-copia-no-painel.md)).

#![allow(unreachable_pub)]

use ir_proto::message::Control;
use ir_proto::version::ProtocolVersion;

use crate::table::{Vector, control, greeting, v};

/// Canal 0 — o que entrou na versão 11.
pub fn control_vectors_v11() -> Vec<Vector> {
    vec![
        v(
            "hello_v11",
            // Versão 11: a amostra de uso para o painel. A saudação só muda no número.
            control(Control::Hello(greeting(ProtocolVersion(11))), 31),
            "00000b0102030405060708090a0b0c0d0e0f100762616e63616461010101010200001f0000",
        ),
        v(
            "panel",
            // O recado do painel é opaco aqui: só o tamanho e os bytes viajam.
            control(Control::Panel(vec![1, 2, 3]), 32),
            "001503010203200000",
        ),
    ]
}
