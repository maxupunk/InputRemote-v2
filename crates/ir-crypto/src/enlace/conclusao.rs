//! Como começar e como terminar um handshake — o que todo portador faz igual.
//!
//! O meio é de cada transporte: a rede reenvia datagramas perdidos, o rádio e o TCP esperam o
//! próximo corpo do *stream*. Mas escolher o padrão Noise pelo modo, e o que se extrai do handshake
//! terminado, é uma coisa só — e escrita três vezes ela já não era exatamente igual.

use crate::error::{CryptoError, Result};
use crate::handshake::Handshake;
use crate::identity::{Identity, PublicKey};
use crate::transport::Transport;

use super::fio::Mode;

/// O que dizer ao par ao iniciar: parear do zero, reconectar com a chave dele fixada, ou trocar as
/// chaves de um enlace de pé.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectMode {
    /// Primeiro pareamento.
    Pair,
    /// Reconexão, com a chave estática do par fixada.
    Reconnect(PublicKey),
    /// Troca de chaves de um enlace que está de pé, com a mesma chave do par. Só a rede usa.
    Rekey(PublicKey),
}

impl ConnectMode {
    /// Com a chave do par, reconecta à identidade fixada; sem ela, pareia do zero.
    #[must_use]
    pub const fn de_chave(chave: Option<PublicKey>) -> Self {
        match chave {
            Some(fixada) => Self::Reconnect(fixada),
            None => Self::Pair,
        }
    }

    /// O modo que vai no fio.
    #[must_use]
    pub const fn modo(self) -> Mode {
        match self {
            Self::Pair => Mode::Pair,
            Self::Reconnect(_) => Mode::Reconnect,
            Self::Rekey(_) => Mode::Rekey,
        }
    }

    /// O handshake do lado de quem inicia.
    ///
    /// # Errors
    ///
    /// [`CryptoError::Handshake`] se o estado do Noise não puder ser montado.
    pub fn iniciar(self, identidade: &Identity) -> Result<Handshake> {
        match self {
            Self::Pair => Handshake::pair_initiator(identidade),
            Self::Reconnect(chave) | Self::Rekey(chave) => {
                Handshake::reconnect_initiator(identidade, chave)
            }
        }
    }
}

impl Mode {
    /// O handshake do lado de quem responde a um corpo com este modo.
    ///
    /// # Errors
    ///
    /// [`CryptoError::Handshake`] se o estado do Noise não puder ser montado.
    pub fn responder(self, identidade: &Identity) -> Result<Handshake> {
        match self {
            Self::Pair => Handshake::pair_responder(identidade),
            Self::Reconnect | Self::Rekey => Handshake::reconnect_responder(identidade),
        }
    }
}

/// O resultado de um handshake bem-sucedido.
#[derive(Debug)]
pub struct Established {
    /// O transporte cifrado, pronto para quadros.
    pub transport: Transport,
    /// A chave estática que o par apresentou.
    pub peer_static: PublicKey,
    /// O código de 6 dígitos, presente só no pareamento.
    pub code: Option<[u8; 6]>,
}

/// Termina um handshake: confere que acabou, e extrai a chave do par, o código e o transporte.
///
/// O código só existe no pareamento — o próprio [`Handshake`] sabe se é um, então quem chama não
/// tem como pedir o código de uma reconexão por engano.
///
/// # Errors
///
/// [`CryptoError::NotFinished`] se o handshake não terminou; [`CryptoError::Handshake`] se o par
/// não apresentou chave estática ou se o Noise recusar a transição.
pub fn concluir(handshake: Handshake) -> Result<Established> {
    if !handshake.is_finished() {
        return Err(CryptoError::NotFinished);
    }
    let peer_static = handshake.remote_static().ok_or(CryptoError::Handshake)?;
    let code = handshake.pairing_code().ok();
    Ok(Established {
        transport: handshake.into_transport()?,
        peer_static,
        code,
    })
}
