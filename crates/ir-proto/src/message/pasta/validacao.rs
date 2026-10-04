//! O que se confere numa mensagem da pasta antes de qualquer uso.
//!
//! Cada caminho é conferido aqui, no crate puro, como os do manifesto: quem grava é o ajudante da
//! pasta, como o usuário, e um `..` vindo da rede escreveria fora da pasta compartilhada.

use super::{Entry, EntryKind, FolderMessage, OpResult};
use crate::error::{ProtoError, Result};
use crate::limits;
use crate::message::data::{is_safe_component, is_safe_relative_path};

/// Valida uma mensagem da pasta compartilhada.
///
/// # Errors
///
/// - [`ProtoError::CountTooLarge`] para mais pastas que [`limits::MAX_KNOWN_FOLDERS`] no `Hello`.
/// - [`ProtoError::TooLarge`] para um bloco acima de [`limits::MAX_FILE_BLOCK`], um pedido acima
///   de [`limits::MAX_RANGE_REQUEST`] ou um nome acima de [`limits::MAX_FOLDER_NAME`].
/// - [`ProtoError::Malformed`] para caminho ou nome inseguro, e para uma leva de mudanças fora da
///   ordem de versão ou além do número que ela diz alcançar.
pub fn validate_folder_message(message: &FolderMessage) -> Result<()> {
    match message {
        FolderMessage::Hello { folders, .. } => contagem(folders.len()),
        FolderMessage::Offer { name, .. } => nome_de_pasta(name),
        FolderMessage::Changes {
            entries,
            up_to,
            last,
            ..
        } => mudancas(entries, *up_to, *last),
        FolderMessage::RequestRange { len, .. } => pedido(*len),
        FolderMessage::Range { data, .. } | FolderMessage::UploadBlock { data, .. } => {
            bloco(data.len())
        }
        FolderMessage::Upload { path, .. }
        | FolderMessage::Delete { path, .. }
        | FolderMessage::CreateDirectory { path, .. } => caminho(path),
        FolderMessage::Rename { from, to, .. } => caminho(from).and_then(|()| caminho(to)),
        FolderMessage::Outcome {
            result: OpResult::Conflict { conflict_path, .. },
            ..
        } => caminho(conflict_path),
        FolderMessage::Copied { paths, .. } => copiados(paths),
        _ => Ok(()),
    }
}

fn copiados(paths: &[String]) -> Result<()> {
    if paths.len() > limits::MAX_COPIED_PATHS {
        return Err(ProtoError::CountTooLarge {
            what: "caminhos copiados",
            actual: paths.len(),
            limit: limits::MAX_COPIED_PATHS,
        });
    }
    paths.iter().try_for_each(|path| caminho(path))
}

fn caminho(path: &str) -> Result<()> {
    if is_safe_relative_path(path) {
        Ok(())
    } else {
        Err(ProtoError::Malformed)
    }
}

fn nome_de_pasta(name: &str) -> Result<()> {
    if name.len() > limits::MAX_FOLDER_NAME {
        return Err(ProtoError::TooLarge {
            actual: name.len(),
            limit: limits::MAX_FOLDER_NAME,
        });
    }
    // `is_safe_component` não recusa `/` porque recebe o caminho já partido nela; o nome não vem
    // partido, e uma barra nele criaria uma subpasta na réplica.
    if !name.contains('/') && is_safe_component(name) {
        Ok(())
    } else {
        Err(ProtoError::Malformed)
    }
}

fn contagem(pastas: usize) -> Result<()> {
    if pastas > limits::MAX_KNOWN_FOLDERS {
        return Err(ProtoError::CountTooLarge {
            what: "pastas anunciadas",
            actual: pastas,
            limit: limits::MAX_KNOWN_FOLDERS,
        });
    }
    Ok(())
}

fn pedido(len: u32) -> Result<()> {
    if len > limits::MAX_RANGE_REQUEST {
        return Err(ProtoError::TooLarge {
            actual: usize::try_from(len).unwrap_or(usize::MAX),
            limit: usize::try_from(limits::MAX_RANGE_REQUEST).unwrap_or(usize::MAX),
        });
    }
    Ok(())
}

fn bloco(len: usize) -> Result<()> {
    if len > limits::MAX_FILE_BLOCK {
        return Err(ProtoError::TooLarge {
            actual: len,
            limit: limits::MAX_FILE_BLOCK,
        });
    }
    Ok(())
}

/// As entradas de uma mensagem vêm em ordem de versão; a última da leva não passa do número que a
/// leva alcança.
///
/// A ordem não é enfeite: a réplica guarda `up_to` como o ponto de onde pedir depois de uma queda,
/// e fora de ordem uma queda no meio da leva a faria pular uma mudança para sempre. Numa mensagem
/// do meio da leva, as entradas da última versão podem passar de `up_to`: é a versão que continua
/// na mensagem seguinte ([`super::changes_messages`]), e por isso ainda não está completa.
fn mudancas(entries: &[Entry], up_to: u64, last: bool) -> Result<()> {
    let mut anterior = 0u64;
    for entrada in entries {
        caminho(&entrada.path)?;
        let subpasta_com_tamanho = entrada.kind == EntryKind::Directory && entrada.size != 0;
        if entrada.version < anterior || subpasta_com_tamanho {
            return Err(ProtoError::Malformed);
        }
        anterior = entrada.version;
    }
    if last && anterior > up_to {
        return Err(ProtoError::Malformed);
    }
    Ok(())
}
