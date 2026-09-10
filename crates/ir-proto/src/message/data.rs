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
    /// Recusa: caminho vazio, absoluto, com componente `..`, com raiz de unidade do Windows,
    /// com byte nulo, ou acima do tamanho máximo. É a defesa contra travessia de diretório,
    /// e ela é feita aqui, no crate puro, onde pode ser testada exaustivamente.
    #[must_use]
    pub fn is_safe_path(&self) -> bool {
        let path = self.path.as_str();
        if path.is_empty() || path.len() > limits::MAX_RELATIVE_PATH {
            return false;
        }
        if path.contains('\0') || path.contains('\\') {
            return false;
        }
        if path.starts_with('/') {
            return false;
        }
        // Raiz de unidade do Windows, como "C:algo".
        if path.as_bytes().get(1) == Some(&b':') {
            return false;
        }
        path.split('/')
            .all(|component| !component.is_empty() && component != ".." && component != ".")
    }
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
mod tests {
    use super::*;

    fn file(path: &str, size: u64) -> ManifestItem {
        ManifestItem {
            path: path.to_owned(),
            size,
            is_dir: false,
        }
    }

    fn dir(path: &str) -> ManifestItem {
        ManifestItem {
            path: path.to_owned(),
            size: 0,
            is_dir: true,
        }
    }

    #[test]
    fn ordinary_relative_paths_are_safe() {
        for path in [
            "a.txt",
            "pasta/a.txt",
            "a/b/c/d.bin",
            "com espaço.txt",
            "acentuação.md",
        ] {
            assert!(file(path, 1).is_safe_path(), "{path:?} deveria ser seguro");
        }
    }

    #[test]
    fn directory_traversal_is_refused() {
        for path in [
            "../fora.txt",
            "a/../../fora.txt",
            "..",
            "a/..",
            "./a.txt",
            "a/./b",
        ] {
            assert!(
                !file(path, 1).is_safe_path(),
                "{path:?} deveria ser recusado"
            );
        }
    }

    #[test]
    fn absolute_and_windows_paths_are_refused() {
        for path in [
            "/etc/passwd",
            "C:/Windows/System32/x.dll",
            "c:x",
            "a\\b",
            "\\\\servidor\\x",
        ] {
            assert!(
                !file(path, 1).is_safe_path(),
                "{path:?} deveria ser recusado"
            );
        }
    }

    #[test]
    fn empty_null_and_oversized_paths_are_refused() {
        assert!(!file("", 1).is_safe_path());
        assert!(!file("a\0b", 1).is_safe_path());
        assert!(!file("a//b", 1).is_safe_path(), "componente vazio");
        let long = "a".repeat(limits::MAX_RELATIVE_PATH + 1);
        assert!(!file(&long, 1).is_safe_path());
    }

    #[test]
    fn a_consistent_manifest_validates() {
        let items = vec![dir("pasta"), file("pasta/a.txt", 10), file("b.bin", 32)];
        assert!(validate_manifest(&items, 42).is_ok());
    }

    #[test]
    fn directories_do_not_count_towards_the_total() {
        let items = vec![dir("pasta"), dir("pasta/sub")];
        assert!(validate_manifest(&items, 0).is_ok());
    }

    #[test]
    fn a_lying_total_is_refused() {
        let items = vec![file("a.txt", 10)];
        assert_eq!(
            validate_manifest(&items, 999).unwrap_err(),
            ProtoError::Malformed
        );
    }

    #[test]
    fn an_overflowing_total_is_refused_without_panicking() {
        let items = vec![file("a.txt", u64::MAX), file("b.txt", 2)];
        assert_eq!(
            validate_manifest(&items, 1).unwrap_err(),
            ProtoError::Malformed
        );
    }

    #[test]
    fn one_unsafe_path_rejects_the_whole_manifest() {
        let items = vec![file("bom.txt", 1), file("../mau.txt", 1)];
        assert_eq!(
            validate_manifest(&items, 2).unwrap_err(),
            ProtoError::Malformed
        );
    }

    #[test]
    fn item_count_is_checked_before_walking_the_list() {
        let items = vec![file("../mau.txt", 0); limits::MAX_MANIFEST_ITEMS + 1];
        let err = validate_manifest(&items, 0).unwrap_err();
        assert!(matches!(
            err,
            ProtoError::CountTooLarge {
                what: "itens do manifesto",
                ..
            }
        ));
    }
}
