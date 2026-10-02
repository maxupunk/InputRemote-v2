//! Os arquivos prometidos, como o Explorer os cola: arquivos virtuais.
//!
//! Dois formatos, os mesmos que a Área de Trabalho Remota usa para colar arquivos que ainda estão
//! do outro lado:
//!
//! - `FileGroupDescriptorW`: a lista — nome, tamanho, pasta ou arquivo. Sai na hora, do manifesto;
//!   é o que o Explorer lê para habilitar o colar e montar o próprio diálogo de cópia.
//! - `FileContents`: o conteúdo de um item, pedido por índice quando a pessoa cola, como um fluxo
//!   que espera os bytes chegarem ([`super::fluxo`]).
//!
//! A diferença para a renderização atrasada, tentada antes e descartada: lá, quem pedia os dados
//! ficava com o clipboard **aberto** enquanto a cópia chegava — e o próprio Explorer pede tudo o que
//! entra no clipboard, então o clipboard da máquina inteira travava, e o colar do Explorer não
//! conseguia abri-lo. Aqui o clipboard só é aberto para pegar este objeto; os bytes chegam pelo
//! fluxo, no diálogo de cópia do Explorer, com o clipboard livre.

#![allow(unsafe_code)]
// O `#[implement]` do `windows` gera `#[inline(always)]` e conversões de referência em ponteiro
// que o clippy pedante aponta; o código é do macro, e não deste módulo.
#![allow(clippy::inline_always, clippy::ref_as_ptr)]

use std::mem::ManuallyDrop;
use std::sync::Arc;

use windows::Win32::Foundation::{
    DATA_S_SAMEFORMATETC, DV_E_FORMATETC, DV_E_LINDEX, E_NOTIMPL, E_POINTER,
    OLE_E_ADVISENOTSUPPORTED, S_OK,
};
use windows::Win32::System::Com::{
    DATADIR_GET, DVASPECT_CONTENT, FORMATETC, IAdviseSink, IDataObject, IDataObject_Impl,
    IEnumFORMATETC, IEnumSTATDATA, IStream, STGMEDIUM, STGMEDIUM_0, TYMED_HGLOBAL, TYMED_ISTREAM,
};
use windows::Win32::System::DataExchange::RegisterClipboardFormatW;
use windows::Win32::UI::Shell::{FILEDESCRIPTORW, SHCreateStdEnumFmtEtc};
use windows::core::{BOOL, HRESULT, Ref, implement, w};

use super::andamento::Andamento;
use super::fluxo::Fluxo;
use crate::chegada::ItemDaChegada;
use crate::error::{ClipError, Result};
use crate::windows::area;

/// Formato que este objeto não oferece.
const FORMATO_ALHEIO: HRESULT = DV_E_FORMATETC;
/// Índice que não é de um arquivo da lista.
const INDICE_ALHEIO: HRESULT = DV_E_LINDEX;

/// O que cada descritor diz de si: atributos, tamanho, nome em UTF-16, e que o Explorer mostre o
/// progresso.
const FD_ATTRIBUTES: u32 = 0x0000_0004;
const FD_FILESIZE: u32 = 0x0000_0040;
const FD_PROGRESSUI: u32 = 0x0000_4000;
const FD_UNICODE: u32 = 0x8000_0000;
const FILE_ATTRIBUTE_DIRECTORY: u32 = 0x10;
const FILE_ATTRIBUTE_NORMAL: u32 = 0x80;

/// `DROPEFFECT_COPY`: colar copia — o que veio do outro computador não sai de lá.
const COPIAR: u32 = 1;

/// Os números dos formatos nesta sessão.
#[derive(Debug, Clone, Copy)]
pub(super) struct Formatos {
    descritor: u16,
    conteudo: u16,
    /// `Preferred DropEffect`: copiar ou mover. Sem ele o Explorer lia a lista, criava as pastas e
    /// não pedia conteúdo nenhum — visto na bancada.
    efeito: u16,
}

impl Formatos {
    pub(super) fn registrar() -> Result<Self> {
        let registrar = |nome| {
            u16::try_from(unsafe { RegisterClipboardFormatW(nome) })
                .ok()
                .filter(|&numero| numero != 0)
                .ok_or(ClipError::Indisponivel(
                    "os formatos de arquivo virtual não registraram",
                ))
        };
        Ok(Self {
            descritor: registrar(w!("FileGroupDescriptorW"))?,
            conteudo: registrar(w!("FileContents"))?,
            efeito: registrar(w!("Preferred DropEffect"))?,
        })
    }

    fn lista(self) -> [FORMATETC; 3] {
        [
            formato(self.descritor, -1, TYMED_HGLOBAL.0),
            formato(self.conteudo, -1, TYMED_ISTREAM.0),
            formato(self.efeito, -1, TYMED_HGLOBAL.0),
        ]
    }
}

fn formato(cf: u16, lindex: i32, tymed: i32) -> FORMATETC {
    FORMATETC {
        cfFormat: cf,
        ptd: core::ptr::null_mut(),
        dwAspect: DVASPECT_CONTENT.0,
        lindex,
        tymed: u32::try_from(tymed).unwrap_or(0),
    }
}

/// O objeto que vai para o clipboard.
#[implement(IDataObject)]
pub(super) struct DadosVirtuais {
    andamento: Arc<Andamento>,
    formatos: Formatos,
}

impl DadosVirtuais {
    pub(super) fn novo(andamento: Arc<Andamento>, formatos: Formatos) -> Self {
        Self {
            andamento,
            formatos,
        }
    }

    fn oferece(&self, pedido: &FORMATETC) -> bool {
        let quer = |tymed: i32| pedido.tymed & u32::try_from(tymed).unwrap_or(0) != 0;
        let em_bloco = [self.formatos.descritor, self.formatos.efeito];
        (em_bloco.contains(&pedido.cfFormat) && quer(TYMED_HGLOBAL.0))
            || (pedido.cfFormat == self.formatos.conteudo && quer(TYMED_ISTREAM.0))
    }
}

impl IDataObject_Impl for DadosVirtuais_Impl {
    fn GetData(&self, pformatetcin: *const FORMATETC) -> windows::core::Result<STGMEDIUM> {
        // SAFETY: ponteiro de entrada do chamador; nulo vira erro.
        let pedido = unsafe { pformatetcin.as_ref() }.ok_or(E_POINTER)?;
        if !self.oferece(pedido) {
            return Err(FORMATO_ALHEIO.into());
        }
        if pedido.cfFormat == self.formatos.descritor {
            let bytes = descritor(self.andamento.itens())
                .map_err(|_| windows::core::Error::from(FORMATO_ALHEIO))?;
            return em_bloco(&bytes);
        }
        if pedido.cfFormat == self.formatos.efeito {
            return em_bloco(&COPIAR.to_le_bytes());
        }
        let indice = usize::try_from(pedido.lindex).map_err(|_| INDICE_ALHEIO)?;
        let fluxo: IStream = Fluxo::novo(Arc::clone(&self.andamento), indice)
            .ok_or(INDICE_ALHEIO)?
            .into();
        Ok(STGMEDIUM {
            tymed: u32::try_from(TYMED_ISTREAM.0).unwrap_or(4),
            u: STGMEDIUM_0 {
                pstm: ManuallyDrop::new(Some(fluxo)),
            },
            pUnkForRelease: ManuallyDrop::new(None),
        })
    }

    fn GetDataHere(
        &self,
        _pformatetc: *const FORMATETC,
        _pmedium: *mut STGMEDIUM,
    ) -> windows::core::Result<()> {
        Err(FORMATO_ALHEIO.into())
    }

    fn QueryGetData(&self, pformatetc: *const FORMATETC) -> HRESULT {
        // SAFETY: ponteiro de entrada do chamador; nulo é recusa.
        match unsafe { pformatetc.as_ref() } {
            Some(pedido) if self.oferece(pedido) => S_OK,
            _ => FORMATO_ALHEIO,
        }
    }

    fn GetCanonicalFormatEtc(
        &self,
        _pformatectin: *const FORMATETC,
        pformatetcout: *mut FORMATETC,
    ) -> HRESULT {
        if !pformatetcout.is_null() {
            // SAFETY: ponteiro de saída do chamador, conferido não nulo.
            unsafe { (*pformatetcout).ptd = core::ptr::null_mut() };
        }
        DATA_S_SAMEFORMATETC
    }

    fn SetData(
        &self,
        _pformatetc: *const FORMATETC,
        _pmedium: *const STGMEDIUM,
        _frelease: BOOL,
    ) -> windows::core::Result<()> {
        // O Explorer conta o resultado da colagem por aqui ("Paste Succeeded"); não há o que
        // guardar, e recusar não atrapalha a cópia.
        Err(E_NOTIMPL.into())
    }

    fn EnumFormatEtc(&self, dwdirection: u32) -> windows::core::Result<IEnumFORMATETC> {
        if dwdirection != u32::try_from(DATADIR_GET.0).unwrap_or(1) {
            return Err(E_NOTIMPL.into());
        }
        unsafe { SHCreateStdEnumFmtEtc(&self.formatos.lista()) }
    }

    fn DAdvise(
        &self,
        _pformatetc: *const FORMATETC,
        _advf: u32,
        _padvsink: Ref<IAdviseSink>,
    ) -> windows::core::Result<u32> {
        Err(OLE_E_ADVISENOTSUPPORTED.into())
    }

    fn DUnadvise(&self, _dwconnection: u32) -> windows::core::Result<()> {
        Err(OLE_E_ADVISENOTSUPPORTED.into())
    }

    fn EnumDAdvise(&self) -> windows::core::Result<IEnumSTATDATA> {
        Err(OLE_E_ADVISENOTSUPPORTED.into())
    }
}

/// Bytes num bloco de memória global, o meio dos formatos que não são fluxo.
fn em_bloco(bytes: &[u8]) -> windows::core::Result<STGMEDIUM> {
    let bloco = area::copiar_para_o_sistema(bytes)
        .map_err(|_| windows::core::Error::from(FORMATO_ALHEIO))?;
    Ok(STGMEDIUM {
        tymed: u32::try_from(TYMED_HGLOBAL.0).unwrap_or(1),
        u: STGMEDIUM_0 { hGlobal: bloco },
        pUnkForRelease: ManuallyDrop::new(None),
    })
}

/// Quanto cabe no nome de um descritor, contando o zero do fim.
const TETO_DO_NOME: usize = 260;

/// O `FILEGROUPDESCRIPTORW` desta lista: a contagem, e um `FILEDESCRIPTORW` por item.
///
/// # Errors
///
/// [`ClipError::FormatoNaoSuportado`] se algum caminho não couber nos 259 caracteres do descritor —
/// aí a cópia não é prometida, e aparece no clipboard quando chegar.
pub(super) fn descritor(itens: &[ItemDaChegada]) -> Result<Vec<u8>> {
    let quantos = u32::try_from(itens.len()).map_err(|_| ClipError::FormatoNaoSuportado)?;
    let mut bytes = Vec::with_capacity(4 + itens.len() * core::mem::size_of::<FILEDESCRIPTORW>());
    bytes.extend_from_slice(&quantos.to_le_bytes());
    for item in itens {
        let descricao = descrever(item)?;
        // SAFETY: `FILEDESCRIPTORW` é `repr(C)` sem ponteiros; os bytes são os que o Explorer lê.
        let crus = unsafe {
            core::slice::from_raw_parts(
                (&raw const descricao).cast::<u8>(),
                core::mem::size_of::<FILEDESCRIPTORW>(),
            )
        };
        bytes.extend_from_slice(crus);
    }
    Ok(bytes)
}

/// O descritor de um item: o caminho relativo com `\`, de onde o Explorer recria as pastas.
pub(super) fn descrever(item: &ItemDaChegada) -> Result<FILEDESCRIPTORW> {
    let nome: Vec<u16> = item.caminho.replace('/', "\\").encode_utf16().collect();
    if nome.len() >= TETO_DO_NOME {
        return Err(ClipError::FormatoNaoSuportado);
    }
    // Montado à parte e copiado inteiro: a struct é compactada, e referência a campo dela não vale.
    let mut nome_fixo = [0u16; TETO_DO_NOME];
    for (destino, unidade) in nome_fixo.iter_mut().zip(nome) {
        *destino = unidade;
    }
    Ok(FILEDESCRIPTORW {
        dwFlags: FD_ATTRIBUTES | FD_FILESIZE | FD_PROGRESSUI | FD_UNICODE,
        dwFileAttributes: if item.pasta {
            FILE_ATTRIBUTE_DIRECTORY
        } else {
            FILE_ATTRIBUTE_NORMAL
        },
        nFileSizeHigh: u32::try_from(item.tamanho >> 32).unwrap_or(u32::MAX),
        nFileSizeLow: u32::try_from(item.tamanho & 0xFFFF_FFFF).unwrap_or(u32::MAX),
        cFileName: nome_fixo,
        ..FILEDESCRIPTORW::default()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(caminho: &str, tamanho: u64, pasta: bool) -> ItemDaChegada {
        ItemDaChegada {
            caminho: caminho.to_owned(),
            tamanho,
            pasta,
        }
    }

    #[test]
    fn o_descritor_tem_o_tamanho_que_o_explorer_espera() {
        assert_eq!(core::mem::size_of::<FILEDESCRIPTORW>(), 592);
        let bytes = descritor(&[item("a", 1, false), item("b", 2, false)]).unwrap();
        assert_eq!(bytes.len(), 4 + 2 * 592);
        assert_eq!(&bytes[..4], &2u32.to_le_bytes());
    }

    #[test]
    fn o_caminho_vai_com_barra_invertida_e_o_tamanho_inteiro() {
        let descricao = descrever(&item("relatório/anexos/b.txt", 5_000_000_000, false)).unwrap();
        let nome_fixo = { descricao.cFileName };
        let nome: Vec<u16> = nome_fixo.iter().copied().take_while(|&u| u != 0).collect();
        assert_eq!(
            String::from_utf16(&nome).unwrap(),
            "relatório\\anexos\\b.txt"
        );
        let tamanho =
            (u64::from(descricao.nFileSizeHigh) << 32) | u64::from(descricao.nFileSizeLow);
        assert_eq!(tamanho, 5_000_000_000);
        let atributos = { descricao.dwFileAttributes };
        assert_eq!(atributos, FILE_ATTRIBUTE_NORMAL);
    }

    #[test]
    fn a_pasta_e_marcada_como_pasta() {
        let descricao = descrever(&item("relatório", 0, true)).unwrap();
        let atributos = { descricao.dwFileAttributes };
        assert_eq!(atributos, FILE_ATTRIBUTE_DIRECTORY);
    }

    #[test]
    fn um_caminho_longo_demais_nao_e_prometido() {
        let longo = "x".repeat(TETO_DO_NOME);
        assert!(descrever(&item(&longo, 1, false)).is_err());
    }
}
