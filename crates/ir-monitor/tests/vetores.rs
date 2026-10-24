//! Vetores gravados do vocabulário do painel, versão 2 — a 1 com os detalhes no fim.
//!
//! O mesmo motivo dos vetores do protocolo (`ir-proto/tests/vectors`): o `postcard` é posicional, e
//! reordenar um campo produz bytes que o outro lado decodifica **com sucesso** e entende errado. O
//! painel e o computador atualizam em momentos diferentes — um pela loja, o outro pelo instalador
//! —, então só um byte gravado pega a mudança esquecida. Mudou de propósito: suba
//! `mensagem::VERSAO` e grave um conjunto novo.

#![allow(clippy::expect_used, clippy::unwrap_used)]

use ir_monitor::mensagem::{codificar, decodificar};
use ir_monitor::{
    Apresentacao, Copia, Detalhes, DoPainel, Enlace, EntrePcs, FaseDaCopia, Interesse,
    InteresseEmProcessos, Maquina, ParaPainel, Processos, Quadro, Rota, Situacao, UsoDeProcesso,
};
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

fn docker() -> UsoDeProcesso {
    UsoDeProcesso {
        nome: "dk".to_owned(),
        instancias: 2,
        cpu_permille: 300,
        memoria_kib: 1024,
    }
}

/// Versão 2: o painel diz o que quer além do quadro, e recebe os detalhes.
#[test]
fn detalhes() {
    let interesse = DoPainel::Interesse(Interesse {
        processos: vec!["dk".to_owned()],
        lista: Some(1),
    });
    confere("interesse", &interesse, "020102646b0101");
    let detalhes = ParaPainel::Detalhes(Detalhes {
        copia: Some(Copia {
            enviando: true,
            nome: "f".to_owned(),
            feitos: 10,
            total: 20,
            por_segundo: Some(5),
            fase: FaseDaCopia::Parada("x".to_owned()),
        }),
        processos: vec![
            Some(Processos {
                vigiados: vec![docker()],
                lista: Vec::new(),
            }),
            None,
        ],
    });
    confere(
        "detalhes",
        &detalhes,
        "02010101660a14010503017802010102646b02ac0280080000",
    );
}

/// O recado entre os computadores, dentro de `Control::Panel`.
#[test]
fn entre_pcs() {
    let confere_recado = |nome: &str, recado: &EntrePcs, gravado: &str| {
        let bytes = ir_monitor::entre_pcs::codificar(recado).unwrap();
        assert_eq!(hex(&bytes), gravado, "o formato de `{nome}` mudou");
        assert_eq!(
            ir_monitor::entre_pcs::decodificar(&bytes).as_ref(),
            Some(recado)
        );
    };
    let assinar = EntrePcs::Assinar(InteresseEmProcessos {
        nomes: vec!["dk".to_owned()],
        lista: true,
    });
    confere_recado("assinar", &assinar, "000102646b01");
    confere_recado("parar", &EntrePcs::Parar, "01");
    let uso = EntrePcs::Uso(
        UsageSample {
            cpu_permille: Some(10),
            ..UsageSample::default()
        },
        Some(Processos {
            vigiados: Vec::new(),
            lista: vec![docker()],
        }),
    );
    confere_recado("uso", &uso, "02010a000000000001000102646b02ac028008");
}
