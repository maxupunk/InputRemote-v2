//! Registrar e desregistrar a pasta como raiz de sincronia.
//!
//! O registro pelo `StorageProviderSyncRootManager` (`WinRT`) é o que põe a pasta no painel lateral do
//! Explorer, com ícone e nome — o que a pessoa reconhece como "o drive". Ele funciona sem pacote
//! MSIX (o registro vai para o HKCU do usuário). Se ele recusar, cai-se no `CfRegisterSyncRoot`:
//! sem a entrada no painel, mas com os arquivos sob demanda, os ícones de estado e o menu — o que
//! importa continua funcionando.

use std::path::Path;

use windows::Storage::Provider::{
    StorageProviderHardlinkPolicy, StorageProviderHydrationPolicy,
    StorageProviderHydrationPolicyModifier, StorageProviderInSyncPolicy,
    StorageProviderPopulationPolicy, StorageProviderSyncRootInfo, StorageProviderSyncRootManager,
};
use windows::Storage::StorageFolder;
use windows::Win32::Storage::CloudFilters::{
    CF_HARDLINK_POLICY_NONE, CF_HYDRATION_POLICY,
    CF_HYDRATION_POLICY_MODIFIER_AUTO_DEHYDRATION_ALLOWED, CF_HYDRATION_POLICY_PROGRESSIVE,
    CF_INSYNC_POLICY_TRACK_ALL, CF_PLACEHOLDER_MANAGEMENT_POLICY_DEFAULT, CF_POPULATION_POLICY,
    CF_POPULATION_POLICY_ALWAYS_FULL, CF_POPULATION_POLICY_MODIFIER_NONE, CF_REGISTER_FLAG_UPDATE,
    CF_SYNC_POLICIES, CF_SYNC_REGISTRATION, CfRegisterSyncRoot, CfUnregisterSyncRoot,
};
use windows::Win32::Storage::FileSystem::{GetVolumeInformationW, GetVolumePathNameW};
use windows::core::{GUID, HSTRING, PCWSTR};

use super::{io, largo, largo_texto};

/// O identificador do provedor, fixo: é como o Windows sabe que todas as raízes são do mesmo.
const PROVEDOR: GUID = GUID::from_u128(0x6a1f_3c2e_8b4d_4e7a_9f10_2c5b_7d8e_9a01);

/// Se a raiz pode ser sob demanda: a Cloud Files API só existe em NTFS.
#[must_use]
pub fn suportado(raiz: &Path) -> bool {
    let caminho = largo(raiz);
    let mut volume = [0u16; 261];
    // SAFETY: o caminho termina em zero e vive até o fim; o buffer de saída é local.
    if unsafe { GetVolumePathNameW(windows::core::PCWSTR(caminho.as_ptr()), &mut volume) }.is_err()
    {
        return false;
    }
    let mut sistema = [0u16; 32];
    // SAFETY: `volume` foi preenchido acima e termina em zero; os outros parâmetros são opcionais.
    let lido = unsafe {
        GetVolumeInformationW(
            PCWSTR(volume.as_ptr()),
            None,
            None,
            None,
            None,
            Some(&mut sistema),
        )
    };
    let fim = sistema
        .iter()
        .position(|c| *c == 0)
        .unwrap_or(sistema.len());
    lido.is_ok() && String::from_utf16_lossy(sistema.get(..fim).unwrap_or_default()) == "NTFS"
}

/// Registra `raiz` como raiz de sincronia, com este identificador e este nome no Explorer.
///
/// Registrar de novo a mesma raiz atualiza o registro: é seguro chamar a cada subida. Devolve o
/// motivo, quando o Explorer recusou e a raiz ficou só na Cloud Files API — sem o painel lateral.
///
/// # Errors
///
/// Quando nem o registro do Explorer nem o da Cloud Files API aceitam a pasta.
pub fn registrar(
    raiz: &Path,
    id: &str,
    nome: &str,
    icone: &str,
) -> std::io::Result<Option<String>> {
    match registrar_no_explorer(raiz, id, nome, icone) {
        Ok(()) => Ok(None),
        Err(erro) => {
            // Sem o painel do Explorer, mas sob demanda do mesmo jeito.
            let motivo = format!("{:#x} {}", erro.code().0, erro.message());
            registrar_so_a_raiz(raiz, id)
                .map(|()| Some(motivo.clone()))
                .map_err(|e| std::io::Error::other(format!("{motivo}; e sem o Explorer: {e}")))
        }
    }
}

fn registrar_no_explorer(
    raiz: &Path,
    id: &str,
    nome: &str,
    icone: &str,
) -> windows::core::Result<()> {
    let info = StorageProviderSyncRootInfo::new()?;
    info.SetId(&HSTRING::from(id))?;
    let pasta = StorageFolder::GetFolderFromPathAsync(&HSTRING::from(raiz.as_os_str()))?.join()?;
    info.SetPath(&pasta)?;
    info.SetDisplayNameResource(&HSTRING::from(nome))?;
    info.SetIconResource(&HSTRING::from(icone))?;
    info.SetHydrationPolicy(StorageProviderHydrationPolicy::Progressive)?;
    info.SetHydrationPolicyModifier(
        StorageProviderHydrationPolicyModifier::AutoDehydrationAllowed,
    )?;
    // Todos os marcadores são criados pelo ajudante, a partir do índice: o Windows não precisa
    // perguntar o que há numa subpasta, e a árvore inteira aparece mesmo offline.
    info.SetPopulationPolicy(StorageProviderPopulationPolicy::AlwaysFull)?;
    info.SetInSyncPolicy(StorageProviderInSyncPolicy::Default)?;
    info.SetHardlinkPolicy(StorageProviderHardlinkPolicy::None)?;
    info.SetShowSiblingsAsGroup(false)?;
    info.SetAllowPinning(true)?;
    info.SetVersion(&HSTRING::from(env!("CARGO_PKG_VERSION")))?;
    StorageProviderSyncRootManager::Register(&info)
}

fn registrar_so_a_raiz(raiz: &Path, id: &str) -> std::io::Result<()> {
    let caminho = largo(raiz);
    let provedor = largo_texto("InputRemote");
    let versao = largo_texto(env!("CARGO_PKG_VERSION"));
    let identidade = id.as_bytes();
    let registro = CF_SYNC_REGISTRATION {
        StructSize: u32::try_from(size_of::<CF_SYNC_REGISTRATION>()).unwrap_or(0),
        ProviderName: PCWSTR(provedor.as_ptr()),
        ProviderVersion: PCWSTR(versao.as_ptr()),
        SyncRootIdentity: identidade.as_ptr().cast(),
        SyncRootIdentityLength: u32::try_from(identidade.len()).unwrap_or(0),
        FileIdentity: std::ptr::null(),
        FileIdentityLength: 0,
        ProviderId: PROVEDOR,
    };
    let politicas = CF_SYNC_POLICIES {
        StructSize: u32::try_from(size_of::<CF_SYNC_POLICIES>()).unwrap_or(0),
        Hydration: CF_HYDRATION_POLICY {
            Primary: CF_HYDRATION_POLICY_PROGRESSIVE,
            Modifier: CF_HYDRATION_POLICY_MODIFIER_AUTO_DEHYDRATION_ALLOWED,
        },
        Population: CF_POPULATION_POLICY {
            Primary: CF_POPULATION_POLICY_ALWAYS_FULL,
            Modifier: CF_POPULATION_POLICY_MODIFIER_NONE,
        },
        InSync: CF_INSYNC_POLICY_TRACK_ALL,
        HardLink: CF_HARDLINK_POLICY_NONE,
        PlaceholderManagement: CF_PLACEHOLDER_MANAGEMENT_POLICY_DEFAULT,
    };
    // SAFETY: os textos e a identidade vivem até o fim da chamada; as estruturas são locais.
    unsafe {
        CfRegisterSyncRoot(
            PCWSTR(caminho.as_ptr()),
            &raw const registro,
            &raw const politicas,
            CF_REGISTER_FLAG_UPDATE,
        )
    }
    .map_err(|e| io(&e))
}

/// Tira do Windows as raízes deste usuário que não são mais de nenhuma pasta — a pasta foi apagada
/// à mão, ou um índice se perdeu. Sem isto, uma pasta nova no mesmo caminho era recusada pelo
/// Explorer ("acesso negado"), porque o caminho continuava registrado com o identificador antigo.
///
/// `prefixo` é o começo dos identificadores deste usuário; `manter`, os das pastas que existem.
pub fn limpar_orfas(prefixo: &str, manter: &[String]) -> usize {
    registradas()
        .into_iter()
        .filter(|id| id.starts_with(prefixo) && !manter.contains(id))
        .filter(|id| {
            StorageProviderSyncRootManager::Unregister(&HSTRING::from(id.as_str())).is_ok()
        })
        .count()
}

/// As raízes registradas, para o teste conferir.
#[cfg(test)]
pub(super) fn registradas_para_teste() -> Vec<String> {
    registradas()
}

/// Os identificadores de todas as raízes registradas nesta máquina.
///
/// Lidos do registro, e não do `GetCurrentSyncRoots`: para um programa sem pacote MSIX ele devolve
/// a lista vazia — visto nesta máquina, com três raízes registradas.
fn registradas() -> Vec<String> {
    use windows::Win32::Foundation::ERROR_SUCCESS;
    use windows::Win32::System::Registry::{
        HKEY, HKEY_LOCAL_MACHINE, KEY_READ, RegCloseKey, RegEnumKeyExW, RegOpenKeyExW,
    };
    let caminho =
        largo_texto(r"SOFTWARE\Microsoft\Windows\CurrentVersion\Explorer\SyncRootManager");
    let mut chave = HKEY::default();
    // SAFETY: o caminho termina em zero; a chave de saída é local e fechada abaixo.
    let aberta = unsafe {
        RegOpenKeyExW(
            HKEY_LOCAL_MACHINE,
            PCWSTR(caminho.as_ptr()),
            None,
            KEY_READ,
            &raw mut chave,
        )
    };
    if aberta != ERROR_SUCCESS {
        return Vec::new();
    }
    let mut ids = Vec::new();
    for indice in 0.. {
        let mut nome = [0u16; 512];
        let mut tamanho = u32::try_from(nome.len()).unwrap_or(0);
        // SAFETY: o buffer e o tamanho são locais; os opcionais vão vazios.
        let lida = unsafe {
            RegEnumKeyExW(
                chave,
                indice,
                Some(windows::core::PWSTR(nome.as_mut_ptr())),
                &raw mut tamanho,
                None,
                None,
                None,
                None,
            )
        };
        if lida != ERROR_SUCCESS {
            break;
        }
        let fim = usize::try_from(tamanho).unwrap_or(0).min(nome.len());
        ids.push(String::from_utf16_lossy(
            nome.get(..fim).unwrap_or_default(),
        ));
    }
    // SAFETY: a chave foi aberta acima e é fechada uma vez.
    let _ = unsafe { RegCloseKey(chave) };
    ids
}

/// Tira a raiz de sincronia do Windows: do painel do Explorer e da Cloud Files API.
pub fn desregistrar(raiz: &Path, id: &str) {
    let _ = StorageProviderSyncRootManager::Unregister(&HSTRING::from(id));
    let caminho = largo(raiz);
    // SAFETY: o caminho termina em zero e vive até o fim da chamada.
    let _ = unsafe { CfUnregisterSyncRoot(PCWSTR(caminho.as_ptr())) };
}
