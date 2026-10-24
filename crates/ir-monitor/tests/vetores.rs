//! Vetores gravados do vocabulário do painel, versão 1.
//!
//! O mesmo motivo dos vetores do protocolo (`ir-proto/tests/vectors`): o `postcard` é posicional, e
//! reordenar um campo produz bytes que o outro lado decodifica **com sucesso** e entende errado. O
//! painel e o computador atualizam em momentos diferentes — um pela loja, o outro pelo instalador
//! —, então só um byte gravado pega a mudança esquecida. Mudou de propósito: suba
//! `mensagem::VERSAO` e grave um conjunto novo.

#![allow(clippy::expect_used, clippy::unwrap_used)]

use ir_monitor::mensagem::{codificar, decodificar};
use ir_monitor::{Apresentacao, DoPainel, Enlace, Maquina, ParaPainel, Quadro, Rota, Situacao};
use ir_proto::message::{Sensor, Temperature, UsageSample};

fn hex(bytes: &[u8]) -> String {
    use core::fmt::Write;
    bytes.iter().fold(String::new(), |mut texto, b| {
        let _ = write!(texto, "{b:02x}");
        texto
    })
}

fn confere<T>(nome: &str, mensagem: &T, gravado: &str)
where
    T: serde::Serialize + serde::de::DeserializeOwned + PartialEq + core::fmt::Debug,
{
    let bytes = codificar(mensagem).unwrap();
    assert_eq!(hex(&bytes), gravado, "o formato de `{nome}` mudou");
    assert_eq!(
        &decodificar::<T>(&bytes).unwrap(),
        mensagem,
        "`{nome}` não voltou igual"
    );
}

#[test]
fn do_painel() {
    let ola = DoPainel::Ola(Apresentacao {
        versao: 1,
        nome: "Tab".to_owned(),
    });
    confere("ola_do_painel", &ola, "000103546162");
    confere("assinar", &DoPainel::Assinar(true), "0101");
}

#[test]
fn para_o_painel() {
    let quadro = ParaPainel::Quadro(Quadro {
        maquinas: vec![
            Maquina {
                nome: "PC".to_owned(),
                situacao: Situacao::Amostra(UsageSample {
                    cpu_permille: Some(231),
                    temperatures: vec![Temperature {
                        sensor: Sensor::Gpu,
                        centi_celsius: 4800,
                    }],
                    ..UsageSample::default()
                }),
            },
            Maquina {
                nome: "nb".to_owned(),
                situacao: Situacao::VersaoAntiga,
            },
        ],
        enlace: Some(Enlace {
            bluetooth_us: Some(7_000),
            rede_us: None,
            vencendo: Some(Rota::Bluetooth),
        }),
    });
    confere(
        "quadro",
        &quadro,
        "01020250430001e701000000000101804b026e62020101d836000100",
    );
}
