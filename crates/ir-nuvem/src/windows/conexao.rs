//! A conexão com a raiz de sincronia: o Windows chama quando um arquivo sob demanda é lido, e o
//! provedor entrega os bytes.
//!
//! O *callback* roda numa thread do Windows e **não espera**: ele só repassa o pedido ao ajudante
//! ([`super::super::Pedido`]) e volta. Os bytes chegam da origem depois, e são entregues por
//! [`entregar`], de outra thread — a Cloud Files API aceita a resposta assíncrona, pela chave de
//! transferência. O Windows espera até 60 s por pedaço; cada entrega recomeça a contagem.

use std::path::{Path, PathBuf};

use windows::Win32::Foundation::NTSTATUS;
use windows::Win32::Storage::CloudFilters::{
    CF_CALLBACK_INFO, CF_CALLBACK_PARAMETERS, CF_CALLBACK_REGISTRATION,
    CF_CALLBACK_TYPE_CANCEL_FETCH_DATA, CF_CALLBACK_TYPE_FETCH_DATA, CF_CALLBACK_TYPE_NONE,
    CF_CONNECT_FLAG_BLOCK_SELF_IMPLICIT_HYDRATION, CF_CONNECT_FLAG_REQUIRE_FULL_FILE_PATH,
    CF_CONNECTION_KEY, CF_OPERATION_INFO, CF_OPERATION_PARAMETERS, CF_OPERATION_PARAMETERS_0,
    CF_OPERATION_PARAMETERS_0_0, CF_OPERATION_TRANSFER_DATA_FLAG_NONE,
    CF_OPERATION_TYPE_TRANSFER_DATA, CfConnectSyncRoot, CfDisconnectSyncRoot, CfExecute,
};
use windows::core::PCWSTR;

use super::{io, largo};
use crate::{Busca, Pedido};

/// O programa que abriu o arquivo ouve "rede indisponível", que é a verdade quando o outro
/// computador não está ao alcance.
const SEM_REDE: NTSTATUS = windows::Win32::Foundation::STATUS_CLOUD_FILE_NETWORK_UNAVAILABLE;

/// Quem recebe os pedidos do Windows, guardado enquanto a conexão viver.
struct Contexto {
    raiz: PathBuf,
    repassar: Box<dyn Fn(Pedido) + Send + Sync>,
}

/// A raiz conectada. Desconecta ao sair de escopo.
pub struct Conexao {
    chave: CF_CONNECTION_KEY,
    contexto: *mut Contexto,
}

// SAFETY: a chave é um número que o Windows aceita de qualquer thread, e o contexto só é lido pelos
// *callbacks* (que exigem `Send + Sync` do que repassam) e liberado uma vez, no `Drop`.
unsafe impl Send for Conexao {}

impl std::fmt::Debug for Conexao {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Conexao")
            .field("chave", &self.chave.0)
            .finish_non_exhaustive()
    }
}

impl Conexao {
    /// A chave da conexão, que acompanha cada pedido.
    #[must_use]
    pub const fn chave(&self) -> i64 {
        self.chave.0
    }
}

impl Drop for Conexao {
    fn drop(&mut self) {
        // SAFETY: a chave veio do `CfConnectSyncRoot`. Depois de desconectar o Windows não chama
        // mais os *callbacks*, e o contexto pode ser liberado.
        let _ = unsafe { CfDisconnectSyncRoot(self.chave) };
        // SAFETY: o ponteiro veio de `Box::into_raw` em `conectar`, e é liberado só aqui.
        drop(unsafe { Box::from_raw(self.contexto) });
    }
}

/// Conecta à raiz: a partir daqui, ler um arquivo sem conteúdo chama `repassar`.
///
/// # Errors
///
/// Quando a pasta não é uma raiz registrada, ou outro processo já a conectou.
pub fn conectar(
    raiz: &Path,
    repassar: impl Fn(Pedido) + Send + Sync + 'static,
) -> std::io::Result<Conexao> {
    let contexto = Box::into_raw(Box::new(Contexto {
        raiz: raiz.to_path_buf(),
        repassar: Box::new(repassar),
    }));
    let tabela = [
        CF_CALLBACK_REGISTRATION {
            Type: CF_CALLBACK_TYPE_FETCH_DATA,
            Callback: Some(ao_buscar),
        },
        CF_CALLBACK_REGISTRATION {
            Type: CF_CALLBACK_TYPE_CANCEL_FETCH_DATA,
            Callback: Some(ao_cancelar),
        },
        CF_CALLBACK_REGISTRATION {
            Type: CF_CALLBACK_TYPE_NONE,
            Callback: None,
        },
    ];
    let caminho = largo(raiz);
    // SAFETY: a tabela termina na entrada `NONE`, como a API pede, e é copiada na chamada. O
    // contexto vive até o `Drop` da conexão. O próprio ajudante não hidrata por acidente: a varredura
    // não lê arquivo sem conteúdo, e `BLOCK_SELF_IMPLICIT_HYDRATION` garante.
    let chave = unsafe {
        CfConnectSyncRoot(
            PCWSTR(caminho.as_ptr()),
            tabela.as_ptr(),
            Some(contexto.cast_const().cast()),
            CF_CONNECT_FLAG_REQUIRE_FULL_FILE_PATH | CF_CONNECT_FLAG_BLOCK_SELF_IMPLICIT_HYDRATION,
        )
    };
    match chave {
        Ok(chave) => Ok(Conexao { chave, contexto }),
        Err(erro) => {
            // SAFETY: a conexão não existiu; ninguém mais tem o ponteiro.
            drop(unsafe { Box::from_raw(contexto) });
            Err(io(&erro))
        }
    }
}

/// O Windows quer bytes de um arquivo.
unsafe extern "system" fn ao_buscar(
    info: *const CF_CALLBACK_INFO,
    parametros: *const CF_CALLBACK_PARAMETERS,
) {
    // SAFETY: o Windows garante os dois ponteiros válidos durante o *callback*.
    let (info, parametros) = unsafe { (&*info, &*parametros) };
    // SAFETY: o contexto é o que `conectar` passou, vivo enquanto a conexão viver.
    let contexto = unsafe { &*info.CallbackContext.cast::<Contexto>() };
    // SAFETY: num FETCH_DATA, a variante do parâmetro é `FetchData`.
    let busca = unsafe { parametros.Anonymous.FetchData };
    // A parte opcional cobre a obrigatória e costuma ser maior: pedir de uma vez poupa idas e voltas.
    let (offset, tamanho) = if busca.OptionalLength > 0 {
        (busca.OptionalFileOffset, busca.OptionalLength)
    } else {
        (busca.RequiredFileOffset, busca.RequiredLength)
    };
    // SAFETY: com `REQUIRE_FULL_FILE_PATH`, o caminho é o completo a partir do volume, terminado em
    // zero, válido durante o *callback*.
    let caminho = unsafe { completo(info) };
    let caminho = crate::relativo(&contexto.raiz, &caminho);
    let (Some(caminho), Ok(offset), Ok(tamanho)) =
        (caminho, u64::try_from(offset), u64::try_from(tamanho))
    else {
        let _ = falhar(info.ConnectionKey.0, info.TransferKey, (0, 0));
        return;
    };
    (contexto.repassar)(Pedido::Buscar(Busca {
        raiz: contexto.raiz.clone(),
        caminho,
        conexao: info.ConnectionKey.0,
        transferencia: info.TransferKey,
        offset,
        tamanho,
    }));
}

/// O Windows desistiu de um pedido.
unsafe extern "system" fn ao_cancelar(
    info: *const CF_CALLBACK_INFO,
    _parametros: *const CF_CALLBACK_PARAMETERS,
) {
    // SAFETY: como em `ao_buscar`.
    let info = unsafe { &*info };
    // SAFETY: como em `ao_buscar`.
    let contexto = unsafe { &*info.CallbackContext.cast::<Contexto>() };
    (contexto.repassar)(Pedido::Cancelar {
        transferencia: info.TransferKey,
    });
}

/// O caminho completo: a letra do volume e o caminho normalizado.
///
/// # Safety
///
/// Os dois textos do `info` têm de estar válidos e terminados em zero.
unsafe fn completo(info: &CF_CALLBACK_INFO) -> PathBuf {
    // SAFETY: repassado de quem chama.
    let volume = unsafe { info.VolumeDosName.to_string() }.unwrap_or_default();
    // SAFETY: repassado de quem chama.
    let resto = unsafe { info.NormalizedPath.to_string() }.unwrap_or_default();
    PathBuf::from(format!("{volume}{resto}"))
}

/// Entrega um pedaço do conteúdo pedido. O `offset` tem de ser múltiplo de 4 KiB, e o tamanho
/// também, salvo o pedaço que chega ao fim do arquivo.
///
/// # Errors
///
/// Quando o Windows recusa: o pedido venceu, ou foi cancelado.
pub fn entregar(
    conexao: i64,
    transferencia: i64,
    offset: u64,
    dados: &[u8],
) -> std::io::Result<()> {
    transferir(conexao, transferencia, NTSTATUS(0), (offset, dados))
}

/// Diz ao Windows que este trecho não vem — o outro computador não está ao alcance. O programa que
/// abriu o arquivo recebe o erro, e o Explorer diz que o provedor não está disponível.
///
/// # Errors
///
/// Quando o Windows recusa a resposta.
pub fn falhar(
    conexao: i64,
    transferencia: i64,
    (offset, tamanho): (u64, u64),
) -> std::io::Result<()> {
    let vazio: [u8; 0] = [];
    let mut resposta = parametros(SEM_REDE, offset, &vazio);
    // O buffer nulo é o que uma falha pede; o tamanho é o do trecho que não vem.
    resposta.Anonymous.TransferData.Length = i64::try_from(tamanho).unwrap_or(0);
    resposta.Anonymous.TransferData.Buffer = std::ptr::null();
    executar(conexao, transferencia, resposta)
}

fn transferir(
    conexao: i64,
    transferencia: i64,
    estado: NTSTATUS,
    (offset, dados): (u64, &[u8]),
) -> std::io::Result<()> {
    executar(conexao, transferencia, parametros(estado, offset, dados))
}

fn parametros(estado: NTSTATUS, offset: u64, dados: &[u8]) -> CF_OPERATION_PARAMETERS {
    CF_OPERATION_PARAMETERS {
        ParamSize: u32::try_from(
            std::mem::offset_of!(CF_OPERATION_PARAMETERS, Anonymous)
                + size_of::<CF_OPERATION_PARAMETERS_0_0>(),
        )
        .unwrap_or(0),
        Anonymous: CF_OPERATION_PARAMETERS_0 {
            TransferData: CF_OPERATION_PARAMETERS_0_0 {
                Flags: CF_OPERATION_TRANSFER_DATA_FLAG_NONE,
                CompletionStatus: estado,
                Buffer: dados.as_ptr().cast(),
                Offset: i64::try_from(offset).unwrap_or(0),
                Length: i64::try_from(dados.len()).unwrap_or(0),
            },
        },
    }
}

fn executar(
    conexao: i64,
    transferencia: i64,
    mut parametros: CF_OPERATION_PARAMETERS,
) -> std::io::Result<()> {
    let operacao = CF_OPERATION_INFO {
        StructSize: u32::try_from(size_of::<CF_OPERATION_INFO>()).unwrap_or(0),
        Type: CF_OPERATION_TYPE_TRANSFER_DATA,
        ConnectionKey: CF_CONNECTION_KEY(conexao),
        TransferKey: transferencia,
        ..Default::default()
    };
    // SAFETY: as estruturas são locais, e o buffer dos dados vive durante a chamada, que copia.
    unsafe { CfExecute(&raw const operacao, &raw mut parametros) }.map_err(|e| io(&e))
}
