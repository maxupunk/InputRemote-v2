//! O handshake Noise conduzido sobre o socket UDP.
//!
//! É a ponte entre o [`ir_crypto::Handshake`] — que só transforma bytes em bytes — e o socket
//! de verdade. Aqui os datagramas de handshake vão e voltam, com o modo em claro no primeiro
//! byte para o respondedor escolher o padrão certo, até o transporte cifrado ficar pronto.

use std::net::SocketAddr;
use std::time::Duration;

use ir_crypto::{Handshake, Identity, PublicKey, Transport};
use tokio::net::UdpSocket;

use crate::error::{NetError, Result};
use crate::wire::{self, Mode};

/// Buffer de recepção de um datagrama de handshake. As mensagens do Noise são pequenas.
const BUF: usize = 2048;

/// Quanto esperar por cada mensagem do par antes de desistir do handshake.
const STEP_TIMEOUT: Duration = Duration::from_millis(1500);

/// O que dizer ao par ao iniciar: parear do zero, ou reconectar com a chave dele fixada.
#[derive(Debug, Clone, Copy)]
pub enum ConnectMode {
    /// Primeiro pareamento.
    Pair,
    /// Reconexão, com a chave estática do par fixada.
    Reconnect(PublicKey),
}

impl ConnectMode {
    const fn wire_mode(self) -> Mode {
        match self {
            Self::Pair => Mode::Pair,
            Self::Reconnect(_) => Mode::Reconnect,
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

/// Conduz o handshake como iniciador, contra `peer`.
///
/// # Errors
///
/// [`NetError::HandshakeTimeout`] se o par não responder; [`NetError::Crypto`] se a criptografia
/// recusar; [`NetError::Io`] em falha de socket.
pub async fn drive_initiator(
    socket: &UdpSocket,
    peer: SocketAddr,
    identity: &Identity,
    mode: ConnectMode,
) -> Result<Established> {
    let handshake = match mode {
        ConnectMode::Pair => Handshake::pair_initiator(identity)?,
        ConnectMode::Reconnect(peer_key) => Handshake::reconnect_initiator(identity, peer_key)?,
    };
    run(
        socket,
        peer,
        handshake,
        mode.wire_mode(),
        matches!(mode, ConnectMode::Pair),
    )
    .await
}

/// Conduz o handshake como respondedor, a partir do primeiro datagrama já recebido.
///
/// `first` é o datagrama que a malha de recepção do endpoint pegou e reconheceu como início de
/// handshake — ele traz o modo e a primeira mensagem Noise.
///
/// # Errors
///
/// Como [`drive_initiator`], mais [`NetError::Malformed`] se o primeiro datagrama não for um
/// handshake válido.
pub async fn drive_responder(
    socket: &UdpSocket,
    peer: SocketAddr,
    identity: &Identity,
    first: &[u8],
) -> Result<Established> {
    let (mode, message) = wire::parse_handshake(first).ok_or(NetError::Malformed)?;
    let mut handshake = match mode {
        Mode::Pair => Handshake::pair_responder(identity)?,
        Mode::Reconnect => Handshake::reconnect_responder(identity)?,
    };
    handshake.read_message(message)?;
    run(socket, peer, handshake, mode, matches!(mode, Mode::Pair)).await
}

/// O laço comum: alterna escrever e ler até o handshake terminar.
async fn run(
    socket: &UdpSocket,
    peer: SocketAddr,
    mut handshake: Handshake,
    mode: Mode,
    is_pairing: bool,
) -> Result<Established> {
    let mut buf = [0u8; BUF];
    // No máximo seis passos cobrem XX (3) e IK (2) com folga, mesmo contando o já lido.
    for _ in 0..6 {
        if handshake.is_finished() {
            break;
        }
        if handshake.is_my_turn() {
            let message = handshake.write_message()?;
            let datagram = wire::handshake_datagram(mode, &message);
            socket.send_to(&datagram, peer).await?;
        } else {
            let datagram = recv_from_peer(socket, peer, &mut buf).await?;
            let (_, message) = wire::parse_handshake(&datagram).ok_or(NetError::Malformed)?;
            handshake.read_message(message)?;
        }
    }

    if !handshake.is_finished() {
        return Err(NetError::Crypto(ir_crypto::CryptoError::NotFinished));
    }

    let peer_static = handshake
        .remote_static()
        .ok_or(NetError::Crypto(ir_crypto::CryptoError::Handshake))?;
    let code = if is_pairing {
        handshake.pairing_code().ok()
    } else {
        None
    };
    let transport = handshake.into_transport()?;
    Ok(Established {
        transport,
        peer_static,
        code,
    })
}

/// Recebe um datagrama do par, ignorando o que vier de outros endereços.
async fn recv_from_peer(socket: &UdpSocket, peer: SocketAddr, buf: &mut [u8]) -> Result<Vec<u8>> {
    loop {
        let recv = tokio::time::timeout(STEP_TIMEOUT, socket.recv_from(buf))
            .await
            .map_err(|_| NetError::HandshakeTimeout)??;
        let (len, from) = recv;
        if from != peer {
            continue; // datagrama de outra origem no meio do handshake
        }
        return Ok(buf.get(..len).unwrap_or(&[]).to_vec());
    }
}
