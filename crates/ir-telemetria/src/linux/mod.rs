//! As fontes do Linux: `/proc` e `/sys`, que o kernel escreve como texto.
//!
//! Os arquivos ficam abertos enquanto a vigia vive e são relidos do começo a cada leitura — sem
//! reabrir a cada segundo ([12, §3.3](../../../../docs/12-painel-android.md)).

mod gpu;
mod proc;
mod processos;
mod sys;

pub(crate) use processos::Proc;

use std::fs::File;
use std::io::{Read, Seek};
use std::path::Path;

use crate::fonte::{Agendada, cadencia};

/// As fontes desta máquina Linux, cada uma na sua cadência.
pub(crate) fn fontes() -> Vec<Agendada> {
    let mut fontes = Vec::new();
    if let Some(fonte) = proc::Processador::abrir() {
        fontes.push(Agendada::nova(fonte, cadencia::RAPIDA));
    }
    if let Some(fonte) = proc::Memoria::abrir() {
        fontes.push(Agendada::nova(fonte, cadencia::RAPIDA));
    }
    if let Some(fonte) = proc::Rede::abrir() {
        fontes.push(Agendada::nova(fonte, cadencia::RAPIDA));
    }
    if let Some(fonte) = sys::Bateria::procurar() {
        fontes.push(Agendada::nova(fonte, cadencia::MEDIA));
    }
    fontes.push(Agendada::nova(sys::Termica::procurar(), cadencia::MEDIA));
    fontes.push(Agendada::nova(sys::Disco, cadencia::LENTA));
    if let Some(placas) = gpu::Placas::procurar() {
        fontes.push(Agendada::nova(placas, cadencia::RAPIDA));
    }
    fontes
}

/// Um arquivo do kernel aberto uma vez e relido do começo a cada leitura.
#[derive(Debug)]
struct Arquivo {
    arquivo: File,
    texto: String,
}

impl Arquivo {
    fn abrir(caminho: impl AsRef<Path>) -> Option<Self> {
        Some(Self {
            arquivo: File::open(caminho).ok()?,
            texto: String::new(),
        })
    }

    /// O conteúdo de agora, no mesmo buffer da leitura anterior.
    fn ler(&mut self) -> Option<&str> {
        self.texto.clear();
        self.arquivo.rewind().ok()?;
        self.arquivo.read_to_string(&mut self.texto).ok()?;
        Some(&self.texto)
    }
}
