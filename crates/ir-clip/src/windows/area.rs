//! O clipboard do Win32, e o único `unsafe` deste crate.
//!
//! Confinado aqui de propósito, como o Winsock do `ir-bt`: acima deste arquivo o resto do produto
//! trabalha com [`Conteudo`], que é Rust seguro ([09, §4](../../../docs/09-padroes-de-codigo.md)).
//!
//! # As regras do recurso, que a API não conta
//!
//! O clipboard do Windows é **global e tem dono**. Três consequências, e todas moram neste arquivo:
//!
//! 1. `OpenClipboard` falha enquanto outro processo o tiver aberto. Não é defeito nosso; é
//!    concorrência normal, e a resposta é tentar de novo ([05, §6](../../../docs/05-windows.md)).
//! 2. Enquanto ele está aberto, **ninguém mais** o usa. Então se fecha o mais rápido possível, e
//!    nada que possa esperar acontece com ele aberto.
//! 3. O que `SetClipboardData` recebe passa a ser **do sistema**: quem chamou não pode liberar nem
//!    tocar depois. É por isso que a memória é `GlobalAlloc`, e não um `Vec`.

#![allow(unsafe_code)]

use std::path::PathBuf;

use windows::Win32::Foundation::{HANDLE, HGLOBAL, HWND};
use windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, GetClipboardData, IsClipboardFormatAvailable, OpenClipboard,
    SetClipboardData,
};
use windows::Win32::System::Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalUnlock};
use windows::Win32::System::Ole::{CF_HDROP, CF_UNICODETEXT};
use windows::Win32::UI::Shell::{DROPFILES, DragQueryFileW, HDROP};

use crate::conteudo::{Conteudo, quebras_nativas};
use crate::error::{ClipError, Result};

/// Quanto cabe num caminho lido do `CF_HDROP`.
///
/// `MAX_PATH` é 260, mas caminho longo existe e chega a 32 767 no Win32. Reservar o teto uma vez é
/// mais barato que truncar o nome de um arquivo do usuário.
const TETO_DE_CAMINHO: usize = 32_768;

/// Abre o clipboard, fecha ao sair de escopo.
///
/// Existe porque há mais de um caminho de erro entre abrir e fechar, e um `CloseClipboard` esquecido
/// num deles **trava o clipboard da máquina inteira** até o processo morrer. `Drop` não esquece.
struct Aberto;

impl Aberto {
    fn agora() -> Result<Self> {
        // `None` como dono: não temos janela para associar, e não precisamos — só lemos e
        // escrevemos, sem renderização atrasada.
        match unsafe { OpenClipboard(Some(HWND::default())) } {
            Ok(()) => Ok(Self),
            Err(_) => Err(ClipError::Ocupado),
        }
    }
}

impl Drop for Aberto {
    fn drop(&mut self) {
        let _ = unsafe { CloseClipboard() };
    }
}

/// O que há no clipboard, se for coisa que o protocolo transporta.
///
/// # Errors
///
/// [`ClipError::Ocupado`] se outro programa o tiver aberto.
pub(super) fn ler() -> Result<Option<Conteudo>> {
    let _aberto = Aberto::agora()?;
    // Arquivos antes de texto: o Explorer põe os dois formatos ao copiar arquivo — `CF_HDROP` com a
    // lista e `CF_UNICODETEXT` com os nomes. Olhar texto primeiro transformaria "copiei um arquivo"
    // em "copiei o nome de um arquivo", que é o defeito mais irritante possível aqui.
    if unsafe { IsClipboardFormatAvailable(CF_HDROP.0.into()) }.is_ok() {
        return ler_arquivos().map(Some);
    }
    if unsafe { IsClipboardFormatAvailable(CF_UNICODETEXT.0.into()) }.is_ok() {
        return ler_texto().map(Some);
    }
    // Formato privado de algum aplicativo. Não é para nós, e não é erro.
    Ok(None)
}

/// Lê `CF_UNICODETEXT` e normaliza as quebras.
fn ler_texto() -> Result<Conteudo> {
    let dados = unsafe { GetClipboardData(CF_UNICODETEXT.0.into()) }
        .map_err(|erro| ClipError::Sistema(erro.to_string()))?;
    let bloco = HGLOBAL(dados.0);
    let ponteiro = unsafe { GlobalLock(bloco) }.cast::<u16>();
    if ponteiro.is_null() {
        return Err(ClipError::Sistema(
            "o bloco de texto não pôde ser travado".to_owned(),
        ));
    }
    // O texto é terminado em zero. Percorre-se até ele, com teto, em vez de confiar em
    // `GlobalSize`: o bloco pode ser maior que o texto.
    let mut unidades = Vec::new();
    let mut i = 0isize;
    loop {
        let unidade = unsafe { *ponteiro.offset(i) };
        if unidade == 0 || unidades.len() >= TETO_DE_CAMINHO * 8 {
            break;
        }
        unidades.push(unidade);
        i += 1;
    }
    let _ = unsafe { GlobalUnlock(bloco) };
    Ok(Conteudo::texto(&String::from_utf16_lossy(&unidades)))
}

/// Lê `CF_HDROP` como lista de caminhos.
fn ler_arquivos() -> Result<Conteudo> {
    let dados = unsafe { GetClipboardData(CF_HDROP.0.into()) }
        .map_err(|erro| ClipError::Sistema(erro.to_string()))?;
    let drop = HDROP(dados.0);
    // `0xFFFF_FFFF` como índice pede a contagem, e não um nome.
    let quantos = unsafe { DragQueryFileW(drop, u32::MAX, None) };
    let mut caminhos = Vec::with_capacity(quantos as usize);
    let mut buffer = vec![0u16; TETO_DE_CAMINHO];
    for indice in 0..quantos {
        let escritos = unsafe { DragQueryFileW(drop, indice, Some(&mut buffer)) } as usize;
        if escritos == 0 {
            continue;
        }
        let Some(nome) = buffer.get(..escritos) else {
            continue;
        };
        caminhos.push(PathBuf::from(String::from_utf16_lossy(nome)));
    }
    Ok(Conteudo::Arquivos(caminhos))
}

/// Põe o conteúdo no clipboard, na forma nativa.
///
/// # Errors
///
/// [`ClipError::Ocupado`]; [`ClipError::FormatoNaoSuportado`] para imagem, que ainda não tem
/// caminho aqui; [`ClipError::Sistema`] em falha de alocação.
pub(super) fn publicar(conteudo: &Conteudo) -> Result<()> {
    let (formato, bytes) = match conteudo {
        Conteudo::Texto(texto) => (CF_UNICODETEXT, bytes_de_texto(texto)),
        Conteudo::Arquivos(caminhos) => (CF_HDROP, bytes_de_arquivos(caminhos)),
        // A imagem é PNG no protocolo e `CF_DIBV5` no Windows, e a conversão é um assunto próprio
        // ([05, §6](../../../docs/05-windows.md)). Recusar é honesto; converter mal não seria.
        Conteudo::Imagem(_) => return Err(ClipError::FormatoNaoSuportado),
    };

    let _aberto = Aberto::agora()?;
    unsafe { EmptyClipboard() }.map_err(|erro| ClipError::Sistema(erro.to_string()))?;
    let bloco = copiar_para_o_sistema(&bytes)?;
    // A partir daqui o bloco é do sistema: não se libera, não se toca. Se `SetClipboardData`
    // falhar, ele ainda é nosso — e aí sim há que soltar.
    match unsafe { SetClipboardData(formato.0.into(), Some(HANDLE(bloco.0))) } {
        Ok(_) => Ok(()),
        Err(erro) => {
            let _ = unsafe { windows::Win32::Foundation::GlobalFree(Some(bloco)) };
            Err(ClipError::Sistema(erro.to_string()))
        }
    }
}

/// O texto como o Windows o quer: UTF-16, com CRLF e terminado em zero.
fn bytes_de_texto(canonico: &str) -> Vec<u8> {
    let nativo = quebras_nativas(canonico);
    let mut unidades: Vec<u16> = nativo.encode_utf16().collect();
    unidades.push(0);
    unidades.iter().flat_map(|u| u.to_le_bytes()).collect()
}

/// A lista de arquivos como o Windows a quer: `DROPFILES`, depois os caminhos em UTF-16 separados
/// por zero, e **dois** zeros no fim.
///
/// O segundo zero é o fim da lista. Sem ele, o Explorer lê memória além do bloco procurando o
/// próximo nome.
fn bytes_de_arquivos(caminhos: &[PathBuf]) -> Vec<u8> {
    let mut nomes: Vec<u16> = Vec::new();
    for caminho in caminhos {
        nomes.extend(caminho.to_string_lossy().encode_utf16());
        nomes.push(0);
    }
    nomes.push(0);

    let cabecalho = DROPFILES {
        pFiles: u32::try_from(core::mem::size_of::<DROPFILES>()).unwrap_or(20),
        pt: windows::Win32::Foundation::POINT { x: 0, y: 0 },
        fNC: false.into(),
        // Os nomes são UTF-16. Marcar como ANSI aqui faria o Explorer ler cada par de bytes como
        // dois caracteres, e o caminho chegaria embaralhado.
        fWide: true.into(),
    };
    let mut bytes = Vec::with_capacity(core::mem::size_of::<DROPFILES>() + nomes.len() * 2);
    // O cabeçalho vai byte a byte, e não por transmutação: a struct é `#[repr(C)]` e o que importa
    // é a sequência de bytes que o Explorer vai ler.
    bytes.extend_from_slice(&cabecalho.pFiles.to_le_bytes());
    bytes.extend_from_slice(&cabecalho.pt.x.to_le_bytes());
    bytes.extend_from_slice(&cabecalho.pt.y.to_le_bytes());
    bytes.extend_from_slice(&i32::from(cabecalho.fNC.as_bool()).to_le_bytes());
    bytes.extend_from_slice(&i32::from(cabecalho.fWide.as_bool()).to_le_bytes());
    bytes.extend(nomes.iter().flat_map(|u| u.to_le_bytes()));
    bytes
}

/// Copia bytes para memória que o clipboard possa adotar.
fn copiar_para_o_sistema(bytes: &[u8]) -> Result<HGLOBAL> {
    let bloco = unsafe { GlobalAlloc(GMEM_MOVEABLE, bytes.len()) }
        .map_err(|erro| ClipError::Sistema(erro.to_string()))?;
    let destino = unsafe { GlobalLock(bloco) }.cast::<u8>();
    if destino.is_null() {
        let _ = unsafe { windows::Win32::Foundation::GlobalFree(Some(bloco)) };
        return Err(ClipError::Sistema(
            "o bloco novo não pôde ser travado".to_owned(),
        ));
    }
    unsafe { core::ptr::copy_nonoverlapping(bytes.as_ptr(), destino, bytes.len()) };
    let _ = unsafe { GlobalUnlock(bloco) };
    Ok(bloco)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn o_texto_sai_em_utf16_com_crlf_e_zero_no_fim() {
        let bytes = bytes_de_texto("a\nb");
        // "a\r\nb\0" em UTF-16 LE.
        assert_eq!(bytes, vec![b'a', 0, b'\r', 0, b'\n', 0, b'b', 0, 0, 0]);
    }

    #[test]
    fn a_lista_de_arquivos_termina_em_dois_zeros() {
        // O defeito que isto previne é leitura fora do bloco pelo Explorer, procurando um nome que
        // não existe.
        let bytes = bytes_de_arquivos(&[PathBuf::from("a"), PathBuf::from("b")]);
        let cauda = &bytes[bytes.len() - 6..];
        assert_eq!(
            cauda,
            &[b'b', 0, 0, 0, 0, 0],
            "nome, zero do nome, zero da lista"
        );
    }

    #[test]
    fn o_cabecalho_tem_o_tamanho_que_ele_mesmo_anuncia() {
        // `pFiles` é o deslocamento onde os nomes começam. Se ele não bater com o tamanho do
        // cabeçalho, o Explorer começa a ler no meio de um caractere.
        let bytes = bytes_de_arquivos(&[PathBuf::from("x")]);
        let anunciado = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as usize;
        assert_eq!(anunciado, core::mem::size_of::<DROPFILES>());
        assert_eq!(anunciado, 20, "DROPFILES são cinco campos de 4 bytes");
    }

    #[test]
    fn o_campo_de_largura_diz_utf16() {
        // Marcar ANSI faria o caminho chegar embaralhado ao Explorer.
        let bytes = bytes_de_arquivos(&[PathBuf::from("x")]);
        let f_wide = i32::from_le_bytes([bytes[16], bytes[17], bytes[18], bytes[19]]);
        assert_eq!(f_wide, 1);
    }

    #[test]
    fn uma_lista_vazia_ainda_e_uma_lista_valida() {
        let bytes = bytes_de_arquivos(&[]);
        assert_eq!(bytes.len(), core::mem::size_of::<DROPFILES>() + 2);
    }
}
