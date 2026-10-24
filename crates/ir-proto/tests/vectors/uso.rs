//! Vetores gravados da amostra de uso para o painel — versão 11.
//!
//! Em arquivo próprio pelo limite de 400 linhas de `table.rs`: o pedido de amostras
//! (`Control::WatchUsage`) e a amostra (`Control::Usage`), com todos os campos preenchidos e com
//! todos vazios — o `None` também é formato de fio.

#![allow(unreachable_pub)]

use ir_proto::message::{Battery, Control, Occupancy, Sensor, Temperature, Traffic, UsageSample};
use ir_proto::version::ProtocolVersion;

use crate::table::{Vector, control, greeting, v};

/// Uma amostra com todos os campos, em números que dá para conferir no hex.
fn full_sample() -> UsageSample {
    UsageSample {
        cpu_permille: Some(231),
        memory: Some(Occupancy {
            used_kib: 11_744_051,
            total_kib: 33_554_432,
        }),
        disk: Some(Occupancy {
            used_kib: 500_000_000,
            total_kib: 1_000_000_000,
        }),
        network: Some(Traffic {
            received_per_s: 1_250_000,
            sent_per_s: 80_000,
        }),
        battery: Some(Battery {
            percent: 76,
            plugged: true,
        }),
        temperatures: vec![
            Temperature {
                sensor: Sensor::Cpu,
                centi_celsius: 6150,
            },
            Temperature {
                sensor: Sensor::Disk,
                centi_celsius: 3900,
            },
        ],
    }
}

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
            "watch_usage",
            control(Control::WatchUsage { watch: true }, 32),
            "001501200000",
        ),
        v(
            "usage_full",
            control(Control::Usage(full_sample()), 33),
            "001601e70101b3e6cc05808080100180cab5ee018094ebdc0301d0a54c80f104014c0102008c6002f83c210000",
        ),
        v(
            "usage_empty",
            control(Control::Usage(UsageSample::default()), 34),
            "0016000000000000220000",
        ),
    ]
}
