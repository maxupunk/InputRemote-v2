//! As placas de vídeo pelo `/sys/class/drm`: o fabricante, o uso e a memória dedicada.
//!
//! O `amdgpu` escreve o uso (`gpu_busy_percent`) e a memória (`mem_info_vram_*`); a Intel e a
//! NVIDIA não escrevem nada disso no `/sys`, e o card delas diz que não há leitura — perguntar ao
//! `nvidia-smi` a cada segundo seria um processo novo por segundo, o oposto do que o painel promete.
//! Os arquivos são achados e abertos uma vez, ao começar a assinatura.

use std::path::Path;
use std::time::Instant;

use ir_monitor::amostra::{Gpu, GpuVendor, Occupancy, UsageSample};

use super::Arquivo;
use crate::fonte::Fonte;
use crate::leitura::numero_do_sys;

/// Uma placa e os arquivos dela.
#[derive(Debug)]
struct Placa {
    fabricante: GpuVendor,
    uso: Option<Arquivo>,
    usada: Option<Arquivo>,
    total_kib: Option<u64>,
}

/// As placas de vídeo desta máquina.
#[derive(Debug)]
pub(super) struct Placas(Vec<Placa>);

impl Placas {
    /// As placas de fabricante conhecido; `None` sem nenhuma — uma máquina virtual, um servidor.
    pub(super) fn procurar() -> Option<Self> {
        let mut placas: Vec<Placa> = std::fs::read_dir("/sys/class/drm")
            .ok()?
            .flatten()
            .filter(|entrada| {
                let nome = entrada.file_name();
                let nome = nome.to_string_lossy();
                // `card0`, e não `card0-HDMI-A-1`, que é uma saída da mesma placa.
                nome.strip_prefix("card")
                    .is_some_and(|n| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()))
            })
            .filter_map(|entrada| placa(&entrada.path().join("device")))
            .collect();
        if placas.is_empty() {
            return None;
        }
        placas.sort_by_key(|p| core::cmp::Reverse(p.total_kib.unwrap_or(0)));
        Some(Self(placas))
    }
}

fn placa(pasta: &Path) -> Option<Placa> {
    let id = std::fs::read_to_string(pasta.join("vendor")).ok()?;
    let fabricante = GpuVendor::from_pci(u32::try_from(numero_do_sys(&id)?).ok()?);
    if fabricante == GpuVendor::Other {
        return None;
    }
    let total_kib = std::fs::read_to_string(pasta.join("mem_info_vram_total"))
        .ok()
        .and_then(|texto| numero_do_sys(&texto))
        .map(|bytes| bytes / 1024);
    Some(Placa {
        fabricante,
        uso: Arquivo::abrir(pasta.join("gpu_busy_percent")),
        usada: Arquivo::abrir(pasta.join("mem_info_vram_used")),
        total_kib,
    })
}

impl Fonte for Placas {
    fn ler(&mut self, _: Instant, amostra: &mut UsageSample) {
        amostra.gpus = self
            .0
            .iter_mut()
            .map(|placa| {
                let busy_permille = placa
                    .uso
                    .as_mut()
                    .and_then(|a| a.ler().and_then(numero_do_sys))
                    .and_then(|porcento| u16::try_from(porcento.min(100) * 10).ok());
                let usada = placa
                    .usada
                    .as_mut()
                    .and_then(|a| a.ler().and_then(numero_do_sys));
                let memory = usada
                    .zip(placa.total_kib)
                    .map(|(bytes, total_kib)| Occupancy {
                        used_kib: bytes / 1024,
                        total_kib,
                    });
                Gpu {
                    vendor: placa.fabricante,
                    busy_permille,
                    memory,
                }
            })
            .collect();
    }
}
