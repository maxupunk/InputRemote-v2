//! Processador, memória, disco, rede e bateria pelo Windows: uma chamada para cada, sem driver.
//!
//! Cada chamada do sistema está embrulhada numa função segura aqui mesmo, e as fontes só usam o
//! embrulho ([09, §4](../../../../docs/09-padroes-de-codigo.md)).

#![allow(unsafe_code)]

use std::time::Instant;

use ir_proto::message::{Battery, Occupancy, Traffic, UsageSample};
use windows_sys::Win32::Foundation::FILETIME;
use windows_sys::Win32::NetworkManagement::IpHelper::{FreeMibTable, GetIfTable2, MIB_IF_TABLE2};
use windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
use windows_sys::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};
use windows_sys::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
use windows_sys::Win32::System::Threading::GetSystemTimes;

use crate::conta::{Taxa, TemposDoProcessador, uso_do_processador};
use crate::fonte::Fonte;

/// O processador, por `GetSystemTimes`.
#[derive(Debug, Default)]
pub(super) struct Processador {
    anterior: Option<TemposDoProcessador>,
}

impl Fonte for Processador {
    fn ler(&mut self, _: Instant, amostra: &mut UsageSample) {
        let Some(agora) = tempos_do_sistema() else {
            return;
        };
        if let Some(antes) = self.anterior.replace(agora) {
            amostra.cpu_permille = uso_do_processador(antes, agora);
        }
    }
}

pub(super) fn tempos_do_sistema() -> Option<TemposDoProcessador> {
    let mut ocioso = FILETIME::default();
    let mut nucleo = FILETIME::default();
    let mut usuario = FILETIME::default();
    // SAFETY: os três ponteiros apontam para `FILETIME` vivos nesta pilha, que a chamada só escreve.
    let deu = unsafe { GetSystemTimes(&raw mut ocioso, &raw mut nucleo, &raw mut usuario) };
    if deu == 0 {
        return None;
    }
    let numero = |f: FILETIME| (u64::from(f.dwHighDateTime) << 32) | u64::from(f.dwLowDateTime);
    // O tempo de núcleo já inclui o ocioso: o total é núcleo mais usuário.
    Some(TemposDoProcessador {
        ocioso: numero(ocioso),
        total: numero(nucleo).saturating_add(numero(usuario)),
    })
}

/// A memória física, por `GlobalMemoryStatusEx`.
#[derive(Debug, Default)]
pub(super) struct Memoria;

impl Fonte for Memoria {
    fn ler(&mut self, _: Instant, amostra: &mut UsageSample) {
        let mut estado = MEMORYSTATUSEX {
            dwLength: u32::try_from(size_of::<MEMORYSTATUSEX>()).unwrap_or(0),
            ..Default::default()
        };
        // SAFETY: `estado` é um `MEMORYSTATUSEX` vivo com `dwLength` preenchido, como a chamada exige.
        if unsafe { GlobalMemoryStatusEx(&raw mut estado) } == 0 {
            return;
        }
        amostra.memory = Some(Occupancy {
            used_kib: (estado.ullTotalPhys.saturating_sub(estado.ullAvailPhys)) / 1024,
            total_kib: estado.ullTotalPhys / 1024,
        });
    }
}

/// O disco do sistema, por `GetDiskFreeSpaceExW` na unidade do Windows.
#[derive(Debug)]
pub(super) struct Disco {
    raiz: Vec<u16>,
}

impl Disco {
    /// A unidade do sistema: `%SystemDrive%\`, e `C:\` se a variável faltar.
    pub(super) fn do_sistema() -> Self {
        let unidade = std::env::var("SystemDrive").unwrap_or_else(|_| "C:".to_owned());
        let raiz = format!("{unidade}\\")
            .encode_utf16()
            .chain(Some(0))
            .collect();
        Self { raiz }
    }
}

impl Fonte for Disco {
    fn ler(&mut self, _: Instant, amostra: &mut UsageSample) {
        let (mut livre_para_mim, mut total, mut livre) = (0u64, 0u64, 0u64);
        // SAFETY: `raiz` termina em zero e vive enquanto `self` vive; os três `u64` são da pilha.
        let deu = unsafe {
            GetDiskFreeSpaceExW(
                self.raiz.as_ptr(),
                &raw mut livre_para_mim,
                &raw mut total,
                &raw mut livre,
            )
        };
        if deu != 0 {
            amostra.disk = Some(Occupancy {
                used_kib: total.saturating_sub(livre) / 1024,
                total_kib: total / 1024,
            });
        }
    }
}

/// O tráfego das placas de rede físicas, por `GetIfTable2`.
#[derive(Debug, Default)]
pub(super) struct Rede {
    recebidos: Taxa,
    enviados: Taxa,
}

impl Fonte for Rede {
    fn ler(&mut self, agora: Instant, amostra: &mut UsageSample) {
        let Some((recebidos, enviados)) = bytes_das_placas() else {
            return;
        };
        let taxas = (
            self.recebidos.anotar(agora, recebidos),
            self.enviados.anotar(agora, enviados),
        );
        if let (Some(received_per_s), Some(sent_per_s)) = taxas {
            amostra.network = Some(Traffic {
                received_per_s,
                sent_per_s,
            });
        }
    }
}

/// `HardwareInterface`, o primeiro bit de `InterfaceAndOperStatusFlags`.
const PLACA_FISICA: u8 = 0b01;
/// `FilterInterface`, o segundo: um filtro do Windows repete a placa embaixo dele, e contá-lo
/// contaria o mesmo pacote duas vezes.
const FILTRO: u8 = 0b10;
/// `IfOperStatusUp`.
const LIGADA: i32 = 1;

fn bytes_das_placas() -> Option<(u64, u64)> {
    let mut tabela: *mut MIB_IF_TABLE2 = core::ptr::null_mut();
    // SAFETY: a chamada aloca a tabela e escreve o ponteiro dela em `tabela`.
    if unsafe { GetIfTable2(&raw mut tabela) } != 0 || tabela.is_null() {
        return None;
    }
    // SAFETY: deu certo, então `tabela` aponta para `NumEntries` linhas contíguas a partir de
    // `Table`, válidas até o `FreeMibTable` abaixo — e a fatia não sai deste bloco.
    let soma = unsafe {
        let linhas = core::slice::from_raw_parts(
            (*tabela).Table.as_ptr(),
            usize::try_from((*tabela).NumEntries).unwrap_or(0),
        );
        linhas
            .iter()
            .filter(|linha| {
                let bits = linha.InterfaceAndOperStatusFlags._bitfield;
                bits & PLACA_FISICA != 0 && bits & FILTRO == 0 && linha.OperStatus == LIGADA
            })
            .fold((0u64, 0u64), |(r, e), linha| {
                (
                    r.saturating_add(linha.InOctets),
                    e.saturating_add(linha.OutOctets),
                )
            })
    };
    // SAFETY: `tabela` veio de `GetIfTable2` e é liberada uma vez só.
    unsafe { FreeMibTable(tabela.cast()) };
    Some(soma)
}

/// A bateria, por `GetSystemPowerStatus`.
#[derive(Debug, Default)]
pub(super) struct Bateria;

/// `BatteryFlag` quando não há bateria.
const SEM_BATERIA: u8 = 128;
/// `BatteryLifePercent` quando a carga é desconhecida.
const CARGA_DESCONHECIDA: u8 = 255;

impl Fonte for Bateria {
    fn ler(&mut self, _: Instant, amostra: &mut UsageSample) {
        let mut estado = SYSTEM_POWER_STATUS::default();
        // SAFETY: `estado` é um `SYSTEM_POWER_STATUS` vivo, que a chamada só escreve.
        if unsafe { GetSystemPowerStatus(&raw mut estado) } == 0 {
            return;
        }
        if estado.BatteryFlag & SEM_BATERIA != 0 || estado.BatteryLifePercent == CARGA_DESCONHECIDA
        {
            return;
        }
        amostra.battery = Some(Battery {
            percent: estado.BatteryLifePercent.min(100),
            plugged: estado.ACLineStatus == 1,
        });
    }
}
