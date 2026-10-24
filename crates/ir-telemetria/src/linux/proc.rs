//! Processador, memória e rede, por `/proc`.

use std::time::Instant;

use ir_proto::message::{Occupancy, Traffic, UsageSample};

use super::Arquivo;
use crate::conta::{Taxa, TemposDoProcessador, uso_do_processador};
use crate::fonte::Fonte;
use crate::leitura;

/// O processador, por `/proc/stat`.
#[derive(Debug)]
pub(super) struct Processador {
    stat: Arquivo,
    anterior: Option<TemposDoProcessador>,
}

impl Processador {
    pub(super) fn abrir() -> Option<Self> {
        Some(Self {
            stat: Arquivo::abrir("/proc/stat")?,
            anterior: None,
        })
    }
}

impl Fonte for Processador {
    fn ler(&mut self, _: Instant, amostra: &mut UsageSample) {
        let Some(agora) = self.stat.ler().and_then(leitura::proc_stat) else {
            return;
        };
        if let Some(antes) = self.anterior.replace(agora) {
            amostra.cpu_permille = uso_do_processador(antes, agora);
        }
    }
}

/// A memória, por `/proc/meminfo`.
#[derive(Debug)]
pub(super) struct Memoria {
    meminfo: Arquivo,
}

impl Memoria {
    pub(super) fn abrir() -> Option<Self> {
        Some(Self {
            meminfo: Arquivo::abrir("/proc/meminfo")?,
        })
    }
}

impl Fonte for Memoria {
    fn ler(&mut self, _: Instant, amostra: &mut UsageSample) {
        if let Some((used_kib, total_kib)) = self.meminfo.ler().and_then(leitura::proc_meminfo) {
            amostra.memory = Some(Occupancy {
                used_kib,
                total_kib,
            });
        }
    }
}

/// O tráfego das interfaces físicas, por `/proc/net/dev`.
#[derive(Debug)]
pub(super) struct Rede {
    dev: Arquivo,
    recebidos: Taxa,
    enviados: Taxa,
}

impl Rede {
    pub(super) fn abrir() -> Option<Self> {
        Some(Self {
            dev: Arquivo::abrir("/proc/net/dev")?,
            recebidos: Taxa::default(),
            enviados: Taxa::default(),
        })
    }
}

impl Fonte for Rede {
    fn ler(&mut self, agora: Instant, amostra: &mut UsageSample) {
        let Some((recebidos, enviados)) = self.dev.ler().and_then(leitura::proc_net_dev) else {
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
