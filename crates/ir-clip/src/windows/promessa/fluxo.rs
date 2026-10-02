//! O conteúdo de um arquivo prometido, lido à medida que ele chega.
//!
//! É o `IStream` que o Explorer lê ao colar arquivos virtuais (`FileContents`). Cada leitura
//! devolve o que já está em disco; se ainda não chegou nada além da posição, espera — é isso que
//! faz a colagem antecipada esperar a cópia em vez de colar pela metade.
//!
//! # Abre, lê e fecha — a cada leitura
//!
//! Medido no Windows: com um arquivo aberto dentro, a pasta da montagem não pode ser renomeada, nem
//! com todo compartilhamento. Segurar o arquivo entre leituras impediria a publicação da entrega.
//! Abrindo só durante a leitura, a janela é de microssegundos, e a publicação repete o `rename` que
//! pegar uma ([`ir_files::staging`](../../../../../ir-files/src/staging.rs)).

#![allow(unsafe_code)]
// O `#[implement]` do `windows` gera `#[inline(always)]` e conversões de referência em ponteiro
// que o clippy pedante aponta; o código é do macro, e não deste módulo.
#![allow(clippy::inline_always, clippy::ref_as_ptr)]

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use windows::Win32::Foundation::{
    E_NOTIMPL, E_POINTER, S_FALSE, S_OK, STG_E_ACCESSDENIED, STG_E_INVALIDFUNCTION, STG_E_READFAULT,
};
use windows::Win32::System::Com::{
    CoTaskMemAlloc, ISequentialStream_Impl, IStream, IStream_Impl, LOCKTYPE, STATFLAG,
    STATFLAG_DEFAULT, STATSTG, STGC, STGM_READ, STGTY_STREAM, STREAM_SEEK, STREAM_SEEK_CUR,
    STREAM_SEEK_END, STREAM_SEEK_SET,
};
use windows::core::{HRESULT, PWSTR, Ref, implement};

use super::andamento::{Andamento, Situacao};

/// De quanto em quanto tempo se olha o arquivo que ainda não tem o próximo bloco.
const ESPERA: Duration = Duration::from_millis(25);

/// Quanto uma leitura espera sem nenhum byte novo antes de desistir.
///
/// O canal de arquivos tem um minuto para voltar quando cai (`ir_transferencia::retomada`); passado
/// isso, a cópia desiste e diz por quê. A leitura não espera mais que ela.
const PRAZO_SEM_PROGRESSO: Duration = Duration::from_secs(60);

/// Um arquivo prometido, lido do começo ao fim.
#[implement(IStream)]
pub(super) struct Fluxo {
    andamento: Arc<Andamento>,
    indice: usize,
    tamanho: u64,
    posicao: Mutex<u64>,
}

impl Fluxo {
    /// O fluxo do item `indice`. `None` se ele não existe ou é pasta.
    pub(super) fn novo(andamento: Arc<Andamento>, indice: usize) -> Option<Self> {
        let item = andamento.itens().get(indice).filter(|item| !item.pasta)?;
        let tamanho = item.tamanho;
        Some(Self {
            andamento,
            indice,
            tamanho,
            posicao: Mutex::new(0),
        })
    }

    fn posicao(&self) -> u64 {
        *self.posicao.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn avancar(&self, lidos: usize) {
        let mut posicao = self.posicao.lock().unwrap_or_else(PoisonError::into_inner);
        *posicao = posicao.saturating_add(u64::try_from(lidos).unwrap_or(u64::MAX));
    }

    /// Lê o próximo pedaço, esperando ele chegar. `Ok(0)` é o fim do arquivo.
    fn ler(&self, destino: &mut [u8]) -> Result<usize, HRESULT> {
        let desde = Instant::now();
        loop {
            let posicao = self.posicao();
            if posicao >= self.tamanho || destino.is_empty() {
                return Ok(0);
            }
            if self.andamento.situacao() == Situacao::Falhou {
                return Err(STG_E_READFAULT);
            }
            let lidos = self.tentar(posicao, destino);
            if lidos > 0 {
                self.avancar(lidos);
                return Ok(lidos);
            }
            if desde.elapsed() > PRAZO_SEM_PROGRESSO {
                return Err(STG_E_READFAULT);
            }
            std::thread::sleep(ESPERA);
        }
    }

    /// Uma tentativa: abre, lê o que houver depois da posição, fecha. Zero se ainda não chegou.
    fn tentar(&self, posicao: u64, destino: &mut [u8]) -> usize {
        let Some(caminho) = self.andamento.caminho(self.indice) else {
            return 0;
        };
        let Ok(mut arquivo) = File::open(&caminho) else {
            return 0; // ainda não criado, ou no meio do `rename` da publicação
        };
        let disponivel = arquivo
            .metadata()
            .map_or(0, |dados| dados.len().min(self.tamanho))
            .saturating_sub(posicao);
        if disponivel == 0 || arquivo.seek(SeekFrom::Start(posicao)).is_err() {
            return 0;
        }
        let cabe = usize::try_from(disponivel)
            .unwrap_or(usize::MAX)
            .min(destino.len());
        destino
            .get_mut(..cabe)
            .and_then(|pedaco| arquivo.read(pedaco).ok())
            .unwrap_or(0)
    }

    /// O nome do arquivo, para quem pedir o `Stat` com nome.
    fn nome(&self) -> String {
        self.andamento
            .itens()
            .get(self.indice)
            .and_then(|item| item.caminho.rsplit('/').next())
            .unwrap_or_default()
            .to_owned()
    }
}

impl ISequentialStream_Impl for Fluxo_Impl {
    fn Read(&self, pv: *mut core::ffi::c_void, cb: u32, pcbread: *mut u32) -> HRESULT {
        if pv.is_null() {
            return E_POINTER;
        }
        let tamanho = usize::try_from(cb).unwrap_or(usize::MAX);
        // SAFETY: o contrato de `ISequentialStream::Read` dá `cb` bytes graváveis em `pv`.
        let destino = unsafe { core::slice::from_raw_parts_mut(pv.cast::<u8>(), tamanho) };
        let (resultado, lidos) = match self.ler(destino) {
            Ok(0) => (S_FALSE, 0),
            Ok(lidos) => (S_OK, lidos),
            Err(erro) => (erro, 0),
        };
        if !pcbread.is_null() {
            // SAFETY: ponteiro de saída do chamador, conferido não nulo.
            unsafe { *pcbread = u32::try_from(lidos).unwrap_or(u32::MAX) };
        }
        resultado
    }

    fn Write(&self, _pv: *const core::ffi::c_void, _cb: u32, _pcbwritten: *mut u32) -> HRESULT {
        STG_E_ACCESSDENIED
    }
}

impl IStream_Impl for Fluxo_Impl {
    fn Seek(
        &self,
        dlibmove: i64,
        dworigin: STREAM_SEEK,
        plibnewposition: *mut u64,
    ) -> windows::core::Result<()> {
        let base = match dworigin {
            STREAM_SEEK_SET => 0,
            STREAM_SEEK_CUR => i128::from(self.posicao()),
            STREAM_SEEK_END => i128::from(self.tamanho),
            _ => return Err(STG_E_INVALIDFUNCTION.into()),
        };
        let nova = u64::try_from(base + i128::from(dlibmove))
            .map_err(|_| windows::core::Error::from(STG_E_INVALIDFUNCTION))?;
        *self.posicao.lock().unwrap_or_else(PoisonError::into_inner) = nova;
        if !plibnewposition.is_null() {
            // SAFETY: ponteiro de saída do chamador, conferido não nulo.
            unsafe { *plibnewposition = nova };
        }
        Ok(())
    }

    fn SetSize(&self, _libnewsize: u64) -> windows::core::Result<()> {
        Err(STG_E_ACCESSDENIED.into())
    }

    fn CopyTo(
        &self,
        _pstm: Ref<IStream>,
        _cb: u64,
        _pcbread: *mut u64,
        _pcbwritten: *mut u64,
    ) -> windows::core::Result<()> {
        Err(E_NOTIMPL.into())
    }

    fn Commit(&self, _grfcommitflags: &STGC) -> windows::core::Result<()> {
        Ok(())
    }

    fn Revert(&self) -> windows::core::Result<()> {
        Ok(())
    }

    fn LockRegion(
        &self,
        _liboffset: u64,
        _cb: u64,
        _dwlocktype: &LOCKTYPE,
    ) -> windows::core::Result<()> {
        Err(STG_E_INVALIDFUNCTION.into())
    }

    fn UnlockRegion(
        &self,
        _liboffset: u64,
        _cb: u64,
        _dwlocktype: u32,
    ) -> windows::core::Result<()> {
        Err(STG_E_INVALIDFUNCTION.into())
    }

    fn Stat(&self, pstatstg: *mut STATSTG, grfstatflag: &STATFLAG) -> windows::core::Result<()> {
        if pstatstg.is_null() {
            return Err(E_POINTER.into());
        }
        let nome = if *grfstatflag == STATFLAG_DEFAULT {
            nome_alocado(&self.nome())
        } else {
            PWSTR::null()
        };
        let estado = STATSTG {
            pwcsName: nome,
            r#type: u32::try_from(STGTY_STREAM.0).unwrap_or(2),
            cbSize: self.tamanho,
            grfMode: STGM_READ,
            ..STATSTG::default()
        };
        // SAFETY: ponteiro de saída do chamador, conferido não nulo.
        unsafe { *pstatstg = estado };
        Ok(())
    }

    fn Clone(&self) -> windows::core::Result<IStream> {
        Err(E_NOTIMPL.into())
    }
}

/// O nome em memória do COM, que é de quem pediu o `Stat` liberar.
fn nome_alocado(nome: &str) -> PWSTR {
    let unidades: Vec<u16> = nome.encode_utf16().chain(Some(0)).collect();
    let bytes = unidades.len() * 2;
    // SAFETY: bloco de `bytes` bytes do alocador do COM, preenchido por inteiro antes de sair.
    unsafe {
        let destino = CoTaskMemAlloc(bytes).cast::<u16>();
        if destino.is_null() {
            return PWSTR::null();
        }
        core::ptr::copy_nonoverlapping(unidades.as_ptr(), destino, unidades.len());
        PWSTR(destino)
    }
}
