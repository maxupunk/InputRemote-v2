//! O handshake Noise conduzido sobre o socket UDP.
//!
//! É a ponte entre o [`ir_crypto::Handshake`] — que só transforma bytes em bytes — e o socket
//! de verdade. Aqui os datagramas de handshake vão e voltam, com o modo em claro no primeiro
//! byte para o respondedor escolher o padrão certo, até o transporte cifrado ficar pronto.

use std::net::SocketAddr;
use std::time::Duration;

use ir_crypto::enlace::concluir;
pub use ir_crypto::enlace::{ConnectMode, Established};
use ir_crypto::{Handshake, Identity};
use tokio::net::UdpSocket;

use crate::error::{NetError, Result};
use crate::wire::{self, Mode};

/// Buffer de recepção de um datagrama de handshake. As mensagens do Noise são pequenas.
const BUF: usize = 2048;

/// Quanto esperar por cada mensagem do par antes de reenviar a nossa — ou de desistir, na
/// reconexão, que o serviço já repete a cada rodada.
const STEP_TIMEOUT: Duration = Duration::from_millis(1500);

/// Quanto um **pareamento** espera o outro lado, reenviando a cada [`STEP_TIMEOUT`].
///
/// Um datagrama perdido era o pareamento perdido: o iniciador mandava a primeira mensagem uma vez,
/// esperava 1,5 s e desistia em silêncio, com a janela dizendo "Aguardando" para sempre. Doze
/// segundos cobrem perda na rede e o outro serviço reiniciando, e ainda cabem na paciência de quem
/// clicou.
const PAREAMENTO_ESPERA: Duration = Duration::from_secs(12);

/// O que um passo de leitura precisa para retransmitir: o que foi enviado por último, o que chegou
/// por último, e até quando esperar.
struct Reenvio<'a> {
    enviado: Option<&'a [u8]>,
    recebido: Option<&'a [u8]>,
    prazo: Duration,
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
    let handshake = mode.iniciar(identity)?;
    run_desde(socket, peer, handshake, mode.modo(), None).await
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
    let mut handshake = mode.responder(identity)?;
    handshake.read_message(message)?;
    run_desde(socket, peer, handshake, mode, Some(first)).await
}

/// O laço comum: alterna escrever e ler até o handshake terminar. `primeiro` é o datagrama que o
/// respondedor já leu, para reconhecer quando ele chegar de novo.
async fn run_desde(
    socket: &UdpSocket,
    peer: SocketAddr,
    mut handshake: Handshake,
    mode: Mode,
    primeiro: Option<&[u8]>,
) -> Result<Established> {
    let prazo = if mode == Mode::Pair {
        PAREAMENTO_ESPERA
    } else {
        STEP_TIMEOUT
    };
    let mut buf = [0u8; BUF];
    let mut enviado: Option<Vec<u8>> = None;
    let mut recebido: Option<Vec<u8>> = primeiro.map(<[u8]>::to_vec);
    // No máximo seis passos cobrem XX (3) e IK (2) com folga, mesmo contando o já lido.
    for _ in 0..6 {
        if handshake.is_finished() {
            break;
        }
        if handshake.is_my_turn() {
            let message = handshake.write_message()?;
            let datagram = wire::handshake_datagram(mode, &message);
            socket.send_to(&datagram, peer).await?;
            enviado = Some(datagram);
        } else {
            let reenvio = Reenvio {
                enviado: enviado.as_deref(),
                recebido: recebido.as_deref(),
                prazo,
            };
            let datagram = receber_reenviando(socket, peer, &mut buf, &reenvio).await?;
            let (_, message) = wire::parse_handshake(&datagram).ok_or(NetError::Malformed)?;
            handshake.read_message(message)?;
            recebido = Some(datagram);
        }
    }

    Ok(concluir(handshake)?)
}

/// Espera a próxima mensagem do par até o prazo, reenviando a nossa a cada [`STEP_TIMEOUT`].
///
/// Um datagrama igual ao último recebido é o par repetindo porque a nossa resposta se perdeu: a
/// resposta vai de novo, e a espera continua. Lê-lo como mensagem nova quebraria o handshake.
async fn receber_reenviando(
    socket: &UdpSocket,
    peer: SocketAddr,
    buf: &mut [u8],
    reenvio: &Reenvio<'_>,
) -> Result<Vec<u8>> {
    let limite = tokio::time::Instant::now() + reenvio.prazo;
    loop {
        let falta = limite.saturating_duration_since(tokio::time::Instant::now());
        if falta.is_zero() {
            return Err(NetError::HandshakeTimeout);
        }
        match recv_from_peer(socket, peer, buf, falta.min(STEP_TIMEOUT)).await {
            Ok(datagram) if Some(datagram.as_slice()) == reenvio.recebido => {}
            Ok(datagram) => return Ok(datagram),
            Err(NetError::HandshakeTimeout) => {}
            Err(outro) => return Err(outro),
        }
        if let Some(enviado) = reenvio.enviado {
            socket.send_to(enviado, peer).await?;
        }
    }
}

/// Recebe um datagrama do par, ignorando o que vier de outros endereços.
async fn recv_from_peer(
    socket: &UdpSocket,
    peer: SocketAddr,
    buf: &mut [u8],
    espera: Duration,
) -> Result<Vec<u8>> {
    loop {
        let recv = tokio::time::timeout(espera, socket.recv_from(buf))
            .await
            .map_err(|_| NetError::HandshakeTimeout)?;
        let (len, from) = match recv {
            Ok(recebido) => recebido,
            // No Windows, um envio a uma porta sem ninguém volta como ICMP "porta inalcançável", e
            // o **próximo** `recv_from` do socket falha com `WSAECONNRESET`. Num socket UDP sem
            // conexão isso não diz nada sobre o par — é o outro serviço ainda não de pé —, e
            // desistir aqui era desistir do pareamento que o reenvio existe para salvar.
            Err(erro) if erro.kind() == std::io::ErrorKind::ConnectionReset => continue,
            Err(erro) => return Err(erro.into()),
        };
        if from != peer {
            continue; // datagrama de outra origem no meio do handshake
        }
        // Um quadro de dados do enlace anterior, ainda em trânsito, não é resposta a este
        // handshake. Tomá-lo por uma derrubava o handshake inteiro — "datagrama malformado" na
        // bancada, a cada tentativa de reconectar depois de uma queda.
        if buf.get(..len).and_then(wire::parse_handshake).is_none() {
            continue;
        }
        return Ok(buf.get(..len).unwrap_or(&[]).to_vec());
    }
}
