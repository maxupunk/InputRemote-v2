//! Leitura e escrita de quadros com prefixo de tamanho, sobre um fluxo assíncrono.
//!
//! O enquadramento é o mesmo do canal de controle sans-io ([`ir_ipc::codec`]): 4 bytes de
//! tamanho em little-endian, seguidos do corpo em `postcard`. Aqui só se acrescenta o passo de
//! E/S — ler os bytes de um socket e escrevê-los —, mantendo a decisão de **quanto** ler onde
//! ela já está, para um anúncio absurdo ser recusado antes de alocar.

use anyhow::Result;
use ir_ipc::codec::{self, PREFIXO};
use serde::Serialize;
use serde::de::DeserializeOwned;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

/// Lê um quadro do fluxo, devolvendo `Ok(None)` no fim limpo (o outro lado fechou).
///
/// # Errors
///
/// Erro de E/S, ou [`ir_ipc::ErroDeCodec`] se o prefixo anunciar um tamanho absurdo ou o corpo
/// não formar a mensagem esperada.
pub(crate) async fn ler<R, T>(fonte: &mut R) -> Result<Option<T>>
where
    R: AsyncRead + Unpin,
    T: DeserializeOwned,
{
    let mut prefixo = [0u8; PREFIXO];
    match fonte.read_exact(&mut prefixo).await {
        Ok(_) => {}
        // Fim de fluxo antes de qualquer byte do próximo quadro: encerramento limpo.
        Err(erro) if erro.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(erro) => return Err(erro.into()),
    }
    // O tamanho é validado contra o limite antes de alocar: o serviço roda privilegiado e não
    // pode ser levado a alocar por um número vindo de fora.
    let tamanho = codec::tamanho_anunciado(&prefixo)?;
    let mut corpo = vec![0u8; tamanho];
    fonte.read_exact(&mut corpo).await?;
    Ok(Some(codec::decodificar(&corpo)?))
}

/// Escreve um quadro no fluxo, com o prefixo de tamanho, e o esvazia.
///
/// # Errors
///
/// Erro de E/S, ou [`ir_ipc::ErroDeCodec`] se a mensagem passar do limite de tamanho.
pub(crate) async fn escrever<W, T>(destino: &mut W, mensagem: &T) -> Result<()>
where
    W: AsyncWrite + Unpin,
    T: Serialize,
{
    let quadro = codec::codificar(mensagem)?;
    destino.write_all(&quadro).await?;
    destino.flush().await?;
    Ok(())
}
