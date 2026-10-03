//! Os marcadores: o arquivo que aparece com nome, tamanho e data, e cujo conteúdo vem quando é lido.

use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
use std::os::windows::io::AsRawHandle;
use std::path::Path;

use windows::Win32::Foundation::HANDLE;
use windows::Win32::Storage::CloudFilters::{
    CF_CONVERT_FLAG_MARK_IN_SYNC, CF_CREATE_FLAG_NONE, CF_DEHYDRATE_FLAG_NONE, CF_FS_METADATA,
    CF_HYDRATE_FLAG_NONE, CF_IN_SYNC_STATE_IN_SYNC,
    CF_PLACEHOLDER_CREATE_FLAG_DISABLE_ON_DEMAND_POPULATION,
    CF_PLACEHOLDER_CREATE_FLAG_MARK_IN_SYNC, CF_PLACEHOLDER_CREATE_INFO, CF_REVERT_FLAG_NONE,
    CF_SET_IN_SYNC_FLAG_NONE, CF_UPDATE_FLAG_DEHYDRATE, CF_UPDATE_FLAG_MARK_IN_SYNC,
    CfConvertToPlaceholder, CfCreatePlaceholders, CfDehydratePlaceholder, CfHydratePlaceholder,
    CfRevertPlaceholder, CfSetInSyncState, CfUpdatePlaceholder,
};
use windows::Win32::Storage::FileSystem::{
    FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_NORMAL, FILE_BASIC_INFO, FILE_FLAG_BACKUP_SEMANTICS,
};
use windows::core::PCWSTR;

use super::{filetime, io, largo, largo_texto};
use crate::Situacao;

/// A identidade gravada em cada marcador. A entrada da origem sai do caminho, pelo índice da
/// réplica; a identidade só precisa existir — a API recusa marcador sem nenhuma.
const IDENTIDADE: &[u8] = b"InputRemote";

const FILE_ATTRIBUTE_PINNED: u32 = 0x0008_0000;
const FILE_ATTRIBUTE_UNPINNED: u32 = 0x0010_0000;
const FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS: u32 = 0x0040_0000;

fn metadados(tamanho: u64, modificado_ns: i64, pasta: bool) -> CF_FS_METADATA {
    let quando = filetime(modificado_ns);
    CF_FS_METADATA {
        BasicInfo: FILE_BASIC_INFO {
            CreationTime: quando,
            LastAccessTime: quando,
            LastWriteTime: quando,
            ChangeTime: quando,
            FileAttributes: if pasta {
                FILE_ATTRIBUTE_DIRECTORY.0
            } else {
                FILE_ATTRIBUTE_NORMAL.0
            },
        },
        FileSize: if pasta {
            0
        } else {
            i64::try_from(tamanho).unwrap_or(i64::MAX)
        },
    }
}

/// Cria o marcador de um arquivo — ou de uma subpasta — em `caminho`, já marcado em dia.
///
/// # Errors
///
/// Quando o pai não está numa raiz de sincronia, ou já existe algo no caminho.
pub fn criar_marcador(
    caminho: &Path,
    tamanho: u64,
    modificado_ns: i64,
    pasta: bool,
) -> std::io::Result<()> {
    let (Some(pai), Some(nome)) = (caminho.parent(), caminho.file_name()) else {
        return Err(std::io::ErrorKind::InvalidInput.into());
    };
    std::fs::create_dir_all(pai)?;
    let base = largo(pai);
    let nome = largo_texto(&nome.to_string_lossy());
    let mut bandeiras = CF_PLACEHOLDER_CREATE_FLAG_MARK_IN_SYNC;
    if pasta {
        bandeiras |= CF_PLACEHOLDER_CREATE_FLAG_DISABLE_ON_DEMAND_POPULATION;
    }
    let mut info = [CF_PLACEHOLDER_CREATE_INFO {
        RelativeFileName: PCWSTR(nome.as_ptr()),
        FsMetadata: metadados(tamanho, modificado_ns, pasta),
        FileIdentity: IDENTIDADE.as_ptr().cast(),
        FileIdentityLength: u32::try_from(IDENTIDADE.len()).unwrap_or(0),
        Flags: bandeiras,
        ..Default::default()
    }];
    let mut feitos = 0u32;
    // SAFETY: os textos e a identidade vivem até o fim da chamada; o vetor é local.
    unsafe {
        CfCreatePlaceholders(
            PCWSTR(base.as_ptr()),
            &mut info,
            CF_CREATE_FLAG_NONE,
            Some(&raw mut feitos),
        )
    }
    .map_err(|e| io(&e))?;
    let [criado] = info;
    criado.Result.ok().map_err(|e| io(&e))
}

/// Põe no marcador a versão nova da origem: tamanho e data novos, e o conteúdo velho fora do
/// disco — ele vem de novo quando for lido.
///
/// Um arquivo fixado ("Sempre manter neste dispositivo") não pode ser esvaziado; ele é trocado por
/// um marcador novo, e quem chama o hidrata de novo.
///
/// # Errors
///
/// Erro de disco ao trocar o arquivo.
pub fn atualizar_marcador(caminho: &Path, tamanho: u64, modificado_ns: i64) -> std::io::Result<()> {
    let metadados = metadados(tamanho, modificado_ns, false);
    let tentativa = abrir(caminho, true).and_then(|arquivo| {
        // SAFETY: o handle é válido enquanto `arquivo` viver; a identidade e os metadados são locais.
        unsafe {
            CfUpdatePlaceholder(
                handle(&arquivo),
                Some(&raw const metadados),
                Some(IDENTIDADE.as_ptr().cast()),
                u32::try_from(IDENTIDADE.len()).unwrap_or(0),
                None,
                CF_UPDATE_FLAG_MARK_IN_SYNC | CF_UPDATE_FLAG_DEHYDRATE,
                None,
                None,
            )
        }
        .map_err(|e| io(&e))
    });
    if tentativa.is_ok() {
        return Ok(());
    }
    std::fs::remove_file(caminho)?;
    criar_marcador(caminho, tamanho, modificado_ns, false)
}

/// Marca o arquivo como em dia com a origem: o ícone vira ✓. Um arquivo que nasceu aqui vira
/// marcador nesse momento.
///
/// # Errors
///
/// Erro de disco, ou o arquivo fora de uma raiz de sincronia.
pub fn marcar_em_dia(caminho: &Path) -> std::io::Result<()> {
    let pasta = std::fs::metadata(caminho)?.is_dir();
    let arquivo = abrir(caminho, true)?;
    // SAFETY: o handle é válido enquanto `arquivo` viver.
    let ja_marcador = unsafe {
        CfSetInSyncState(
            handle(&arquivo),
            CF_IN_SYNC_STATE_IN_SYNC,
            CF_SET_IN_SYNC_FLAG_NONE,
            None,
        )
    };
    if ja_marcador.is_ok() {
        return Ok(());
    }
    let _ = pasta;
    // SAFETY: como acima; a identidade é estática.
    unsafe {
        CfConvertToPlaceholder(
            handle(&arquivo),
            Some(IDENTIDADE.as_ptr().cast()),
            u32::try_from(IDENTIDADE.len()).unwrap_or(0),
            CF_CONVERT_FLAG_MARK_IN_SYNC,
            None,
            None,
        )
    }
    .map_err(|e| io(&e))
}

/// Traz o conteúdo inteiro — para "Sempre manter neste dispositivo". Bloqueia até chegar: chame
/// numa thread própria, porque quem entrega os bytes é o laço do ajudante.
///
/// # Errors
///
/// Quando o conteúdo não vem — o outro computador não está ao alcance.
pub fn hidratar(caminho: &Path) -> std::io::Result<()> {
    let arquivo = abrir(caminho, false)?;
    // SAFETY: o handle é válido enquanto `arquivo` viver.
    unsafe { CfHydratePlaceholder(handle(&arquivo), 0, -1, CF_HYDRATE_FLAG_NONE, None) }
        .map_err(|e| io(&e))
}

/// Tira o conteúdo do disco e deixa o marcador — "Liberar espaço".
///
/// # Errors
///
/// Quando o arquivo está aberto, ou mudou e ainda não foi para a origem.
pub fn desidratar(caminho: &Path) -> std::io::Result<()> {
    let arquivo = abrir(caminho, true)?;
    // SAFETY: o handle é válido enquanto `arquivo` viver.
    unsafe { CfDehydratePlaceholder(handle(&arquivo), 0, -1, CF_DEHYDRATE_FLAG_NONE, None) }
        .map_err(|e| io(&e))
}

/// Devolve o arquivo a arquivo comum, com o conteúdo que tem — para quando a pasta deixa de ser
/// compartilhada.
///
/// # Errors
///
/// Erro de disco.
pub fn reverter(caminho: &Path) -> std::io::Result<()> {
    let arquivo = abrir(caminho, true)?;
    // SAFETY: o handle é válido enquanto `arquivo` viver.
    unsafe { CfRevertPlaceholder(handle(&arquivo), CF_REVERT_FLAG_NONE, None) }.map_err(|e| io(&e))
}

/// O que os atributos dizem de um arquivo numa raiz de sincronia.
#[must_use]
pub fn situacao(dados: &std::fs::Metadata) -> Situacao {
    let atributos = dados.file_attributes();
    Situacao {
        sem_conteudo: atributos & FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS != 0,
        fixado: atributos & FILE_ATTRIBUTE_PINNED != 0,
        liberar: atributos & FILE_ATTRIBUTE_UNPINNED != 0,
    }
}

/// Abre sem ler o conteúdo — o que não traz o arquivo da origem. Subpasta abre com a semântica de
/// cópia de segurança, a única forma de o Windows dar handle de pasta.
fn abrir(caminho: &Path, escrita: bool) -> std::io::Result<std::fs::File> {
    let mut opcoes = std::fs::OpenOptions::new();
    if escrita {
        opcoes.write(true);
    } else {
        opcoes.read(true);
    }
    opcoes
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS.0)
        .open(caminho)
}

fn handle(arquivo: &std::fs::File) -> HANDLE {
    HANDLE(arquivo.as_raw_handle())
}
