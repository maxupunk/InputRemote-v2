//! Temperaturas e bateria, por `/sys`; o disco do sistema, por `statvfs`.

#![allow(unsafe_code)]

use std::ffi::CString;
use std::path::Path;
use std::time::Instant;

use ir_proto::message::{Battery, Occupancy, Sensor, UsageSample};

use super::Arquivo;
use crate::fonte::Fonte;
use crate::leitura;

/// Quantos `tempN_input` olhar em cada `hwmon`. Os drivers comuns têm até uns dez (um por núcleo
/// no `coretemp`); dezesseis é folga e põe teto na varredura.
const MAX_ENTRADAS_POR_HWMON: u32 = 16;

/// As temperaturas dos `hwmon` de drivers conhecidos ([`leitura::sensor_do_hwmon`]).
///
/// A varredura de `/sys/class/hwmon` é feita uma vez, ao começar a assinatura: os arquivos de
/// temperatura ficam abertos, e cada leitura só os relê.
#[derive(Debug)]
pub(super) struct Termica {
    entradas: Vec<(Sensor, Arquivo)>,
}

impl Termica {
    pub(super) fn procurar() -> Self {
        let mut entradas = Vec::new();
        let Ok(dispositivos) = std::fs::read_dir("/sys/class/hwmon") else {
            return Self { entradas };
        };
        for dispositivo in dispositivos.flatten() {
            let pasta = dispositivo.path();
            let Some(sensor) = std::fs::read_to_string(pasta.join("name"))
                .ok()
                .and_then(|nome| leitura::sensor_do_hwmon(&nome))
            else {
                continue;
            };
            entradas.extend(temperaturas_em(&pasta).map(|arquivo| (sensor, arquivo)));
        }
        Self { entradas }
    }
}

fn temperaturas_em(pasta: &Path) -> impl Iterator<Item = Arquivo> + '_ {
    (1..=MAX_ENTRADAS_POR_HWMON)
        .filter_map(move |n| Arquivo::abrir(pasta.join(format!("temp{n}_input"))))
}

impl Fonte for Termica {
    fn ler(&mut self, _: Instant, amostra: &mut UsageSample) {
        let leituras = self.entradas.iter_mut().filter_map(|(sensor, arquivo)| {
            let centi = arquivo.ler().and_then(leitura::milesimos_para_centesimos)?;
            Some((*sensor, centi))
        });
        amostra.temperatures = leitura::mais_quentes(leituras);
    }
}

/// A bateria de `/sys/class/power_supply`, quando há uma.
#[derive(Debug)]
pub(super) struct Bateria {
    capacidade: Arquivo,
    estado: Arquivo,
}

impl Bateria {
    pub(super) fn procurar() -> Option<Self> {
        let pasta = std::fs::read_dir("/sys/class/power_supply")
            .ok()?
            .flatten()
            .map(|entrada| entrada.path())
            .find(|pasta| e_bateria(pasta))?;
        Some(Self {
            capacidade: Arquivo::abrir(pasta.join("capacity"))?,
            estado: Arquivo::abrir(pasta.join("status"))?,
        })
    }
}

fn e_bateria(pasta: &Path) -> bool {
    std::fs::read_to_string(pasta.join("type")).is_ok_and(|tipo| tipo.trim() == "Battery")
}

impl Fonte for Bateria {
    fn ler(&mut self, _: Instant, amostra: &mut UsageSample) {
        let Some(capacidade) = self.capacidade.ler().map(str::to_owned) else {
            return;
        };
        let Some(estado) = self.estado.ler() else {
            return;
        };
        if let Some((percent, plugged)) = leitura::bateria(&capacidade, estado) {
            amostra.battery = Some(Battery { percent, plugged });
        }
    }
}

/// O disco em que está a raiz, por `statvfs("/")`.
#[derive(Debug)]
pub(super) struct Disco;

impl Fonte for Disco {
    fn ler(&mut self, _: Instant, amostra: &mut UsageSample) {
        let Ok(raiz) = CString::new("/") else {
            return;
        };
        let mut info: libc::statvfs = unsafe_zeroed();
        // SAFETY: `raiz` termina em zero e vive até o fim da chamada; `info` é da pilha, e a chamada
        // só a escreve.
        if unsafe { libc::statvfs(raiz.as_ptr(), &raw mut info) } != 0 {
            return;
        }
        // `fsblkcnt_t` e `c_ulong` são `u64` em 64 bits e `u32` nos ARM de 32 bits: a conversão é
        // necessária lá, e só parece inútil aqui.
        #[allow(clippy::useless_conversion)]
        let (bloco, blocos, livres) = (
            u64::from(info.f_frsize),
            u64::from(info.f_blocks),
            u64::from(info.f_bfree),
        );
        let total = blocos.saturating_mul(bloco);
        let livre = livres.saturating_mul(bloco);
        amostra.disk = Some(Occupancy {
            used_kib: total.saturating_sub(livre) / 1024,
            total_kib: total / 1024,
        });
    }
}

fn unsafe_zeroed() -> libc::statvfs {
    // SAFETY: `statvfs` é uma struct C de inteiros, para a qual todos os bits zero são um valor
    // válido.
    unsafe { std::mem::zeroed() }
}
