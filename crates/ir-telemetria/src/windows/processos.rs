//! A foto dos processos pelo Windows: **uma** chamada, `NtQuerySystemInformation`, que devolve
//! todos os processos com o tempo de processador e a memória de cada um.
//!
//! A primeira versão abria cada processo (`OpenProcess` + `GetProcessTimes` +
//! `K32GetProcessMemoryInfo`): com 400 processos e a máquina ocupada, a foto levava de 0,3 a 1,8 s,
//! a amostra do painel atrasava e a ligação caía por silêncio (log 70). Esta é a mesma chamada do
//! Gerenciador de Tarefas: alguns milissegundos, sem abrir processo nenhum.
//!
//! Cada chamada do sistema está embrulhada numa função segura aqui mesmo
//! ([09, §4](../../../../docs/09-padroes-de-codigo.md)).

#![allow(unsafe_code)]

use windows_sys::Wdk::System::SystemInformation::{
    NtQuerySystemInformation, SystemProcessInformation,
};
use windows_sys::Win32::System::WindowsProgramming::SYSTEM_PROCESS_INFORMATION;

use super::sistema::tempos_do_sistema;
use crate::processos::{Bruto, Foto, Fotografo};

/// `STATUS_INFO_LENGTH_MISMATCH`: o buffer não coube; a chamada diz quanto precisa.
const NAO_COUBE: i32 = 0xC000_0004_u32.cast_signed();

/// A folga sobre o tamanho pedido: entre uma chamada e outra nascem processos.
const FOLGA: usize = 64 * 1024;

/// Onde ficam, dentro de `Reserved1`, os campos que o `windows-sys` não nomeia. O layout é o do
/// `SYSTEM_PROCESS_INFORMATION` completo (`phnt`, `ntexapi.h`), estável desde o Windows 7: logo
/// depois de `NumberOfThreads` vêm `WorkingSetPrivateSize` (8 bytes), `HardFaultCount`,
/// `NumberOfThreadsHighWatermark` (4 + 4), `CycleTime`, `CreateTime`, `UserTime` e `KernelTime`
/// (8 cada). O `sysinfo` e o Process Hacker leem os mesmos campos.
const PRIVADA: usize = 0;
const TEMPO_DE_USUARIO: usize = 32;
const TEMPO_DE_NUCLEO: usize = 40;

/// A lista de processos do sistema, num buffer que fica de uma foto para a outra.
#[derive(Debug, Default)]
pub(crate) struct ListaDoSistema {
    buffer: Vec<u64>,
}

impl Fotografo for ListaDoSistema {
    fn fotografar(&mut self) -> Option<Foto> {
        let total = tempos_do_sistema()?.total;
        let bytes = self.consultar()?;
        Some(Foto {
            total,
            processos: ler(bytes),
        })
    }
}

impl ListaDoSistema {
    /// Os bytes da lista. O buffer cresce até caber, e fica do tamanho da última vez.
    fn consultar(&mut self) -> Option<&[u8]> {
        for _ in 0..4 {
            let capacidade = u32::try_from(self.buffer.len() * 8).ok()?;
            let mut usado = 0u32;
            // SAFETY: o ponteiro e o tamanho são os do `buffer`, vivo e exclusivo durante a chamada,
            // que só escreve nele até `capacidade` bytes; `usado` é da pilha.
            let estado = unsafe {
                NtQuerySystemInformation(
                    SystemProcessInformation,
                    self.buffer.as_mut_ptr().cast(),
                    capacidade,
                    &raw mut usado,
                )
            };
            if estado == NAO_COUBE || (estado >= 0 && self.buffer.is_empty()) {
                let preciso = usize::try_from(usado).ok()? + FOLGA;
                self.buffer.resize(preciso.div_ceil(8), 0);
                continue;
            }
            if estado < 0 {
                return None;
            }
            let usado = usize::try_from(usado).ok()?.min(self.buffer.len() * 8);
            // SAFETY: `u64` para `u8` é só outra vista dos mesmos bytes, sem exigência de alinhamento.
            let bytes =
                unsafe { core::slice::from_raw_parts(self.buffer.as_ptr().cast::<u8>(), usado) };
            return Some(bytes);
        }
        None
    }
}

/// Percorre a lista encadeada por `NextEntryOffset`, conferindo cada pedaço contra o tamanho dos
/// bytes antes de ler — um deslocamento torto acaba a leitura, e não sai da memória.
fn ler(bytes: &[u8]) -> Vec<Bruto> {
    let tamanho = size_of::<SYSTEM_PROCESS_INFORMATION>();
    let mut processos = Vec::with_capacity(512);
    let mut posicao = 0usize;
    while let Some(pedaco) = bytes.get(posicao..posicao + tamanho) {
        // SAFETY: `pedaco` tem exatamente o tamanho da struct, que é feita só de inteiros e
        // ponteiros (qualquer bit vale); a leitura sem alinhamento não exige nada do endereço.
        let entrada = unsafe {
            pedaco
                .as_ptr()
                .cast::<SYSTEM_PROCESS_INFORMATION>()
                .read_unaligned()
        };
        if let Some(processo) = bruto(&entrada, bytes) {
            processos.push(processo);
        }
        match usize::try_from(entrada.NextEntryOffset) {
            Ok(0) | Err(_) => break,
            Ok(proxima) => posicao += proxima,
        }
    }
    processos
}

fn bruto(entrada: &SYSTEM_PROCESS_INFORMATION, bytes: &[u8]) -> Option<Bruto> {
    let pid = u32::try_from(entrada.UniqueProcessId as usize).ok()?;
    // O processo ocioso (pid 0) não é processo: é o tempo que sobra.
    if pid == 0 {
        return None;
    }
    let campo = |de: usize| {
        let pedaco = entrada.Reserved1.get(de..de + 8)?;
        Some(u64::from_ne_bytes(pedaco.try_into().ok()?))
    };
    let tempo = campo(TEMPO_DE_USUARIO)?.saturating_add(campo(TEMPO_DE_NUCLEO)?);
    let memoria_kib = campo(PRIVADA)? / 1024;
    Some(Bruto {
        pid,
        nome: nome(entrada, bytes)?,
        tempo,
        memoria_kib,
    })
}

/// O nome do executável, sem o `.exe`. O texto mora dentro dos mesmos bytes, apontado pela entrada:
/// o ponteiro é conferido contra os limites deles antes de qualquer leitura.
fn nome(entrada: &SYSTEM_PROCESS_INFORMATION, bytes: &[u8]) -> Option<String> {
    let comprimento = usize::from(entrada.ImageName.Length);
    let inicio = (entrada.ImageName.Buffer as usize).checked_sub(bytes.as_ptr() as usize)?;
    let texto = bytes.get(inicio..inicio.checked_add(comprimento)?)?;
    let unidades: Vec<u16> = texto
        .chunks_exact(2)
        .map(|par| <[u8; 2]>::try_from(par).map_or(0, u16::from_ne_bytes))
        .collect();
    let nome = String::from_utf16_lossy(&unidades);
    Some(sem_exe(nome))
}

fn sem_exe(nome: String) -> String {
    match nome.len().checked_sub(4) {
        Some(corte)
            if nome
                .get(corte..)
                .is_some_and(|ext| ext.eq_ignore_ascii_case(".exe")) =>
        {
            nome.get(..corte).unwrap_or_default().to_owned()
        }
        _ => nome,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn o_nome_perde_o_exe() {
        assert_eq!(sem_exe("Docker Desktop.exe".to_owned()), "Docker Desktop");
        assert_eq!(sem_exe("VmmemWSL".to_owned()), "VmmemWSL");
        assert_eq!(sem_exe("x.EXE".to_owned()), "x");
    }

    /// O tempo da foto não é conferido aqui: numa máquina ocupada o relógio de parede mede a fila
    /// do escalonador, e não a chamada. A medida está no log 70.
    #[test]
    fn a_foto_desta_maquina_tem_este_processo() {
        let foto = ListaDoSistema::default().fotografar().unwrap();
        assert!(foto.total > 0);
        let eu = foto.processos.iter().find(|p| p.pid == std::process::id());
        assert!(
            eu.is_some_and(|p| p.memoria_kib > 0 && p.tempo > 0 && !p.nome.is_empty()),
            "{eu:?}"
        );
    }

    #[test]
    fn bytes_tortos_acabam_a_leitura_sem_sair_deles() {
        assert!(ler(&[]).is_empty());
        assert!(ler(&[0xFF; 16]).is_empty());
    }
}
