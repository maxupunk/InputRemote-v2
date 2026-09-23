//! Canal 5 — dados. Imagens, arquivos e clipboard grande. Só sobre TCP.
//!
//! Aqui integridade vale mais que latência, o oposto exato dos canais de entrada. Um
//! documento não vale nada se perder um byte, e ninguém nota se ele chegou 200 ms depois.
//! Por isso: BLAKE3 por item, confirmação de volta para a origem, e cota conferida **antes**
//! de materializar qualquer coisa (`docs/01-visao-e-escopo.md` §3.3).

use serde::{Deserialize, Serialize};

use crate::error::{ProtoError, Result};
use crate::limits;

/// Identificador de uma transferência.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct TransferId(pub u32);

/// Um item anunciado num manifesto.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManifestItem {
    /// Caminho relativo à raiz da transferência, sempre com `/`.
    ///
    /// Validado por [`ManifestItem::is_safe_path`] antes de qualquer uso. Um caminho vindo
    /// da rede que contenha `..` ou seja absoluto é uma tentativa de escrever fora da pasta
    /// de destino, e este código roda com privilégio.
    pub path: String,
    /// Tamanho em bytes. Zero para diretório.
    pub size: u64,
    /// Se é diretório.
    pub is_dir: bool,
}

impl ManifestItem {
    /// Se o caminho é seguro para materializar.
    ///
    /// Recusa: caminho vazio, absoluto, com componente `..`, com byte nulo, ou acima do tamanho
    /// máximo; e, em **qualquer** componente, o que o Windows interpreta em vez de gravar (ver
    /// [`is_safe_component`]). É a defesa contra travessia de diretório, e ela é feita aqui, no
    /// crate puro, onde pode ser testada exaustivamente.
    #[must_use]
    pub fn is_safe_path(&self) -> bool {
        let path = self.path.as_str();
        if path.is_empty() || path.len() > limits::MAX_RELATIVE_PATH {
            return false;
        }
        if path.starts_with('/') {
            return false;
        }
        path.split('/').all(is_safe_component)
    }
}

/// Se um componente de caminho vira exatamente um nome de arquivo, nos dois sistemas.
///
/// O destino pode ser Windows, e lá o serviço grava como SYSTEM. Um `:` em qualquer componente,
/// e não só no começo, é raiz de unidade (`x/C:payload.dll` vira `C:payload.dll`, relativo à
/// pasta de trabalho do serviço, que é `System32`) ou fluxo alternativo (`a:fluxo`). Nomes de
/// dispositivo (`CON`, `NUL`, `COM1`…) abrem o dispositivo, e o Windows apaga ponto e espaço do
/// fim, o que faz dois nomes diferentes caírem no mesmo arquivo. Os caracteres proibidos pelo
/// Windows e os de controle também ficam de fora: o que não pode ser gravado lá não viaja.
#[must_use]
pub fn is_safe_component(component: &str) -> bool {
    if component.is_empty() || component == "." || component == ".." {
        return false;
    }
    if component.ends_with('.') || component.ends_with(' ') {
        return false;
    }
    let proibido =
        |c: char| c.is_control() || matches!(c, '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|');
    if component.chars().any(proibido) {
        return false;
    }
    !is_windows_device_name(component)
}

/// Se o nome, ignorando a extensão, é um dispositivo do Windows (`nul.txt` também abre `NUL`).
fn is_windows_device_name(component: &str) -> bool {
    let stem = component.split('.').next().unwrap_or(component).trim_end();
    let upper = stem.to_ascii_uppercase();
    if matches!(
        upper.as_str(),
        "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$"
    ) {
        return true;
    }
    let mut chars = upper.chars();
    let prefix: String = chars.by_ref().take(3).collect();
    let rest: String = chars.collect();
    matches!(prefix.as_str(), "COM" | "LPT")
        && matches!(
            rest.as_str(),
            "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³"
        )
}

/// Mensagem do canal de dados.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum BulkMessage {
    /// O que a transferência contém, antes de mandar qualquer byte.
    ///
    /// Permite ao destino conferir cota e espaço em disco e recusar de imediato, em vez de
    /// descobrir no meio e deixar uma árvore parcial.
    Manifest {
        /// Identificador desta transferência.
        id: TransferId,
        /// Os itens.
        items: Vec<ManifestItem>,
        /// Soma dos tamanhos, para conferência rápida.
        total_bytes: u64,
    },
    /// O destino aceita a transferência.
    Accept {
        /// A transferência aceita.
        id: TransferId,
    },
    /// O destino recusa, com motivo.
    Reject {
        /// A transferência recusada.
        id: TransferId,
        /// Por quê.
        reason: RejectReason,
    },
    /// Começo de um arquivo do manifesto.
    FileStart {
        /// A transferência.
        id: TransferId,
        /// Índice do item no manifesto.
        item: u32,
    },
    /// Um bloco de conteúdo.
    FileBlock {
        /// A transferência.
        id: TransferId,
        /// Índice do item no manifesto.
        item: u32,
        /// Deslocamento do bloco dentro do arquivo.
        offset: u64,
        /// Os bytes.
        data: Vec<u8>,
    },
    /// Fim de um arquivo, com o resumo para conferir.
    FileEnd {
        /// A transferência.
        id: TransferId,
        /// Índice do item no manifesto.
        item: u32,
        /// BLAKE3 do conteúdo completo.
        hash: [u8; 32],
    },
    /// O destino confere e responde.
    ///
    /// A confirmação de volta é o que permite a origem dizer "chegou" em vez de o usuário
    /// ficar em dúvida — uma das poucas coisas que o v1 acertou e que se mantém.
    Verified {
        /// A transferência.
        id: TransferId,
        /// Índice do item.
        item: u32,
        /// Se o resumo conferiu.
        ok: bool,
    },
    /// Progresso, para a interface.
    Progress {
        /// A transferência.
        id: TransferId,
        /// Bytes já recebidos, no total.
        bytes_done: u64,
    },
    /// Cancelamento, de qualquer um dos lados.
    ///
    /// Quem recebe apaga o *staging* inteiro. Não existe transferência meio feita virando
    /// arquivo no destino.
    Cancel {
        /// A transferência.
        id: TransferId,
        /// Por quê.
        reason: CancelReason,
    },
}

/// Por que uma transferência foi recusada antes de começar.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum RejectReason {
    /// Passa da cota configurada.
    OverQuota,
    /// Não há espaço em disco.
    NoDiskSpace,
    /// O manifesto tem item com caminho inseguro.
    UnsafePath,
    /// O manifesto tem mais itens que o limite.
    TooManyItems,
    /// O usuário não autorizou receber arquivos.
    NotPermitted,
}

/// Por que uma transferência em curso foi interrompida.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum CancelReason {
    /// O usuário cancelou.
    UserRequested,
    /// O resumo não conferiu.
    HashMismatch,
    /// Erro de escrita no destino.
    WriteFailed,
    /// O canal de dados caiu.
    ///
    /// **Não** derruba a entrada: teclado e mouse continuam pelo portador deles
    /// (`docs/02-arquitetura.md` §8).
    LinkLost,
    /// Uma cópia mais nova substituiu esta.
    Superseded,
}

/// Valida um manifesto antes de qualquer materialização.
///
/// # Errors
///
/// - [`ProtoError::CountTooLarge`] acima de [`limits::MAX_MANIFEST_ITEMS`], conferido antes
///   de percorrer a lista.
/// - [`ProtoError::Malformed`] para caminho inseguro, ou quando `total_bytes` não confere
///   com a soma dos itens.
pub fn validate_manifest(items: &[ManifestItem], total_bytes: u64) -> Result<()> {
    if items.len() > limits::MAX_MANIFEST_ITEMS {
        return Err(ProtoError::CountTooLarge {
            what: "itens do manifesto",
            actual: items.len(),
            limit: limits::MAX_MANIFEST_ITEMS,
        });
    }
    if items.iter().any(|item| !item.is_safe_path()) {
        return Err(ProtoError::Malformed);
    }
    let declared: Option<u64> = items
        .iter()
        .filter(|i| !i.is_dir)
        .try_fold(0u64, |acc, i| acc.checked_add(i.size));
    match declared {
        Some(sum) if sum == total_bytes => Ok(()),
        _ => Err(ProtoError::Malformed),
    }
}

#[cfg(test)]
mod tests;
