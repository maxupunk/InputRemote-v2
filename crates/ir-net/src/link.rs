//! O enlace cifrado já estabelecido: cifra ao enviar, decifra ao receber.
//!
//! Segura o [`Transport`] e o endereço do par. Quem lê do socket é o endpoint; este tipo só
//! transforma um texto claro com espécie num datagrama pronto, e um datagrama recebido de volta
//! na espécie e no conteúdo.

use std::net::SocketAddr;
use std::sync::Arc;

use ir_crypto::Transport;
use tokio::net::UdpSocket;

use crate::error::{NetError, Result};
use crate::wire::{self, Kind};

/// Um enlace cifrado com o par.
#[derive(Debug)]
pub struct SecureLink {
    socket: Arc<UdpSocket>,
    peer: SocketAddr,
    transport: Transport,
}

impl SecureLink {
    /// Monta o enlace a partir do transporte que o handshake produziu.
    #[must_use]
    pub fn new(socket: Arc<UdpSocket>, peer: SocketAddr, transport: Transport) -> Self {
        Self {
            socket,
            peer,
            transport,
        }
    }

    /// O endereço do par.
    #[must_use]
    pub const fn peer(&self) -> SocketAddr {
        self.peer
    }

    /// Cifra e envia um texto claro da espécie dada.
    ///
    /// # Errors
    ///
    /// [`NetError::Crypto`] se a cifragem falhar; [`NetError::Io`] em falha de socket.
    pub async fn send(&mut self, kind: Kind, payload: &[u8]) -> Result<()> {
        let plaintext = wire::wrap(kind, payload);
        let (counter, ciphertext) = self.transport.seal(&plaintext)?;
        let datagram = wire::data_datagram(counter, &ciphertext);
        self.socket.send_to(&datagram, self.peer).await?;
        Ok(())
    }

    /// Abre um datagrama de dados recebido, devolvendo a espécie e o conteúdo.
    ///
    /// # Errors
    ///
    /// [`NetError::Malformed`] se o datagrama não tiver o formato de dados; [`NetError::Crypto`]
    /// se a decifragem ou a janela de repetição recusarem.
    pub fn open(&mut self, datagram: &[u8]) -> Result<(Kind, Vec<u8>)> {
        let (counter, ciphertext) = wire::parse_data(datagram).ok_or(NetError::Malformed)?;
        let plaintext = self.transport.open(counter, ciphertext)?;
        let (kind, payload) = wire::unwrap(&plaintext).ok_or(NetError::Malformed)?;
        Ok((kind, payload.to_vec()))
    }
}
