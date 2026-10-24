//! A foto dos processos pelo `/proc`: um `stat` por processo, e o `/proc/stat` para o total.

#![allow(unsafe_code)]

use std::fs;

use super::Arquivo;
use crate::leitura;
use crate::processos::{Bruto, Foto, Fotografo};

/// Lê `/proc/<pid>/stat` de cada processo. O `/proc/stat` fica aberto; os dos processos, não —
/// eles nascem e morrem entre uma foto e outra.
#[derive(Debug)]
pub(crate) struct Proc {
    stat: Arquivo,
    pagina_kib: u64,
}

impl Proc {
    pub(crate) fn abrir() -> Option<Self> {
        Some(Self {
            stat: Arquivo::abrir("/proc/stat")?,
            pagina_kib: tamanho_da_pagina() / 1024,
        })
    }
}

impl Fotografo for Proc {
    fn fotografar(&mut self) -> Option<Foto> {
        let total = self.stat.ler().and_then(leitura::proc_stat)?.total;
        let processos = fs::read_dir("/proc")
            .ok()?
            .filter_map(Result::ok)
            .filter_map(|entrada| entrada.file_name().to_str()?.parse::<u32>().ok())
            .filter_map(|pid| {
                // Um processo que acabou entre listar e ler some da foto, e só.
                let texto = fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
                let (nome, tempo, paginas) = leitura::proc_pid_stat(&texto)?;
                Some(Bruto {
                    pid,
                    nome: nome.to_owned(),
                    tempo,
                    memoria_kib: paginas.saturating_mul(self.pagina_kib),
                })
            })
            .collect();
        Some(Foto { total, processos })
    }
}

fn tamanho_da_pagina() -> u64 {
    // SAFETY: `sysconf` só lê uma constante do sistema.
    let bytes = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
    u64::try_from(bytes).unwrap_or(4096)
}
