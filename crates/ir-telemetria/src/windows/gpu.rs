//! As placas de vídeo pelo Windows: o fabricante e o LUID pelo DXGI, e o uso e a memória pelas
//! estatísticas do próprio núcleo de vídeo (`D3DKMTQueryStatistics`).
//!
//! A primeira versão usava os contadores de desempenho (`\GPU Engine(*)\Utilization Percentage`):
//! eles enumeram um item por processo em cada motor, e cada leitura levava de 0,5 a 0,9 s
//! ([log 72](../../../../docs/logs/72-gpu-e-todos-os-sensores.md)). As estatísticas dão o tempo de
//! cada motor e a memória de cada segmento **da placa**, sem passar pelos processos: alguns
//! microssegundos por motor. É o caminho do Process Hacker.
//!
//! O DXGI é lido uma vez, quando a vigia começa: as placas não mudam com a máquina ligada. Cada
//! chamada do sistema está embrulhada numa função segura aqui mesmo
//! ([09, §4](../../../../docs/09-padroes-de-codigo.md)).

#![allow(unsafe_code)]

use std::time::Instant;

use ir_monitor::amostra::{Gpu, GpuVendor, Occupancy, UsageSample};
use windows::Win32::Graphics::Dxgi::{
    CreateDXGIFactory1, DXGI_ADAPTER_FLAG_SOFTWARE, IDXGIFactory1,
};
use windows_sys::Wdk::Graphics::Direct3D::{
    D3DKMT_QUERYSTATISTICS, D3DKMT_QUERYSTATISTICS_ADAPTER, D3DKMT_QUERYSTATISTICS_NODE,
    D3DKMT_QUERYSTATISTICS_SEGMENT, D3DKMT_QUERYSTATISTICS_TYPE, D3DKMTQueryStatistics,
};
use windows_sys::Win32::Foundation::LUID;

use crate::fonte::Fonte;
use crate::gpu::uso_dos_motores;

/// Abaixo disto a placa é integrada: a "memória dedicada" dela é um pedaço reservado da
/// principal, e mostrar "120 MB de 128 MB" seria um alarme falso.
const DEDICADA_MINIMA: u64 = 256 * 1024 * 1024;

/// O tempo dos motores conta em 100 ns.
const TIQUES_POR_SEGUNDO: f64 = 10_000_000.0;

/// Uma placa de vídeo de verdade (o renderizador por software do Windows fica de fora).
struct Placa {
    luid: LUID,
    fabricante: GpuVendor,
    dedicada: u64,
    motores: u32,
    segmentos: u32,
    /// Os tempos de cada motor na leitura anterior, e quando.
    anterior: Option<(Instant, Vec<u64>)>,
}

impl core::fmt::Debug for Placa {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Placa")
            .field("fabricante", &self.fabricante)
            .field("dedicada", &self.dedicada)
            .field("motores", &self.motores)
            .finish_non_exhaustive()
    }
}

/// As placas desta máquina.
#[derive(Debug)]
pub(super) struct Placas(Vec<Placa>);

impl Placas {
    /// As placas, ou `None` numa máquina sem placa de vídeo que o DXGI conte.
    pub(super) fn procurar() -> Option<Self> {
        let mut placas = placas_do_dxgi();
        if placas.is_empty() {
            return None;
        }
        // A de mais memória primeiro: num notebook com duas, é a dedicada, a que importa.
        placas.sort_by_key(|p| core::cmp::Reverse(p.dedicada));
        Some(Self(placas))
    }
}

impl Fonte for Placas {
    fn ler(&mut self, agora: Instant, amostra: &mut UsageSample) {
        amostra.gpus = self.0.iter_mut().map(|placa| placa.ler(agora)).collect();
    }
}

impl Placa {
    fn ler(&mut self, agora: Instant) -> Gpu {
        let tempos: Vec<u64> = (0..self.motores)
            .map(|motor| tempo_do_motor(self.luid, motor).unwrap_or(0))
            .collect();
        let busy_permille = self.anterior.as_ref().and_then(|(quando, antes)| {
            let segundos = agora.saturating_duration_since(*quando).as_secs_f64();
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let intervalo = (segundos * TIQUES_POR_SEGUNDO) as u64;
            uso_dos_motores(antes, &tempos, intervalo)
        });
        self.anterior = Some((agora, tempos));
        let memory = (self.dedicada >= DEDICADA_MINIMA).then(|| Occupancy {
            used_kib: memoria_em_uso(self.luid, self.segmentos) / 1024,
            total_kib: self.dedicada / 1024,
        });
        Gpu {
            vendor: self.fabricante,
            busy_permille,
            memory,
        }
    }
}

fn placas_do_dxgi() -> Vec<Placa> {
    // SAFETY: sem argumentos; devolve uma interface COM contada, solta pelo `Drop` dela.
    let Ok(fabrica) = (unsafe { CreateDXGIFactory1::<IDXGIFactory1>() }) else {
        return Vec::new();
    };
    let software = u32::try_from(DXGI_ADAPTER_FLAG_SOFTWARE.0).unwrap_or(0);
    (0..8)
        // SAFETY: a fábrica é válida; um índice além do último devolve erro, e a enumeração para.
        .map_while(|i| unsafe { fabrica.EnumAdapters1(i) }.ok())
        // SAFETY: a placa veio da fábrica e é válida durante a chamada.
        .filter_map(|placa| unsafe { placa.GetDesc1() }.ok())
        .filter(|desc| desc.Flags & software == 0)
        .filter_map(|desc| {
            let luid = LUID {
                LowPart: desc.AdapterLuid.LowPart,
                HighPart: desc.AdapterLuid.HighPart,
            };
            let (motores, segmentos) = contagem(luid)?;
            Some(Placa {
                luid,
                fabricante: GpuVendor::from_pci(desc.VendorId),
                dedicada: u64::try_from(desc.DedicatedVideoMemory).unwrap_or(0),
                motores,
                segmentos,
                anterior: None,
            })
        })
        .collect()
}

/// Uma consulta às estatísticas da placa; o que volta depende do tipo pedido.
fn consultar(
    luid: LUID,
    tipo: D3DKMT_QUERYSTATISTICS_TYPE,
    preparar: impl FnOnce(&mut D3DKMT_QUERYSTATISTICS),
) -> Option<D3DKMT_QUERYSTATISTICS> {
    let mut consulta = D3DKMT_QUERYSTATISTICS {
        Type: tipo,
        AdapterLuid: luid,
        ..Default::default()
    };
    preparar(&mut consulta);
    // SAFETY: a consulta é uma struct viva desta pilha, com o tipo e a placa preenchidos; a
    // chamada só escreve o resultado nela.
    let estado = unsafe { D3DKMTQueryStatistics(&raw const consulta) };
    (estado >= 0).then_some(consulta)
}

/// Quantos motores e quantos segmentos de memória a placa tem.
fn contagem(luid: LUID) -> Option<(u32, u32)> {
    let consulta = consultar(luid, D3DKMT_QUERYSTATISTICS_ADAPTER, |_| {})?;
    // SAFETY: o tipo pedido foi o da placa, então é este o campo da união que veio preenchido.
    let placa = unsafe { consulta.QueryResult.AdapterInformation };
    Some((placa.NodeCount, placa.NbSegments))
}

/// O tempo, em 100 ns, que este motor passou trabalhando desde que a máquina ligou.
fn tempo_do_motor(luid: LUID, motor: u32) -> Option<u64> {
    let consulta = consultar(luid, D3DKMT_QUERYSTATISTICS_NODE, |c| {
        c.Anonymous.QueryNode.NodeId = motor;
    })?;
    // SAFETY: o tipo pedido foi o do motor, então é este o campo da união que veio preenchido.
    let tempo = unsafe {
        consulta
            .QueryResult
            .NodeInformation
            .GlobalInformation
            .RunningTime
    };
    u64::try_from(tempo).ok()
}

/// Os bytes residentes nos segmentos da própria placa. Os de "abertura" são a memória principal
/// emprestada, e não entram — é a conta da "memória dedicada" do Gerenciador de Tarefas.
fn memoria_em_uso(luid: LUID, segmentos: u32) -> u64 {
    (0..segmentos)
        .filter_map(|segmento| {
            let consulta = consultar(luid, D3DKMT_QUERYSTATISTICS_SEGMENT, |c| {
                c.Anonymous.QuerySegment.SegmentId = segmento;
            })?;
            // SAFETY: o tipo pedido foi o do segmento, então é este o campo da união preenchido.
            let info = unsafe { consulta.QueryResult.SegmentInformation };
            (info.Aperture == 0).then_some(info.BytesResident)
        })
        .sum()
}
