//! O vocabulário do endpoint UDP: o que o serviço pede, e o que o endpoint conta.
//!
//! Separado de [`crate::endpoint`] por tamanho, e porque é o contrato que o serviço enxerga — a
//! máquina de estados do endpoint não precisa ser lida para usá-lo.

use std::net::SocketAddr;

use ir_crypto::PublicKey;

use crate::handshake::ConnectMode;

/// O que o serviço manda ao endpoint.
#[derive(Debug)]
#[non_exhaustive]
pub enum NetCommand {
    /// Comece a conectar, como iniciador.
    Connect {
        /// O endereço do par.
        peer: SocketAddr,
        /// Parear do zero ou reconectar com chave fixada.
        mode: ConnectMode,
    },
    /// Mande este quadro (bytes já codificados de `ir_proto::Frame`) ao par.
    SendFrame(Vec<u8>),
    /// O usuário respondeu à comparação de códigos.
    ConfirmPairing(bool),
    /// Se um pedido de pareamento que chega de fora é atendido.
    ///
    /// O serviço desliga isto quando já há par e a janela de pareamento não está aberta. Sem a
    /// chave, qualquer um na rede local mandava um handshake de pareamento a cada poucos segundos:
    /// o endpoint ia esperar uma confirmação que nunca vinha, deixava de atender a reconexão do
    /// par de verdade, e ainda punha um código na tela de ninguém (log 45).
    AcceptPairing(bool),
    /// Encerre o enlace atual.
    Disconnect,
    /// Nos testes: troque as chaves no próximo envio, sem esperar um milhão de quadros.
    #[cfg(test)]
    ForcarRechave,
    /// Encerre a tarefa.
    Shutdown,
}

/// O que o endpoint conta ao serviço.
#[derive(Debug)]
#[non_exhaustive]
pub enum NetEvent {
    /// O handshake de pareamento terminou; aqui está o código para o usuário comparar.
    PairingCode {
        /// Os seis dígitos.
        code: [u8; 6],
        /// A chave estática que o par apresentou, para gravar após a confirmação.
        peer_static: PublicKey,
        /// O endereço do par.
        peer: SocketAddr,
    },
    /// O enlace está pronto: pareamento confirmado dos dois lados, ou reconexão fixada.
    Established {
        /// A chave estática do par.
        peer_static: PublicKey,
        /// O endereço do par.
        peer: SocketAddr,
    },
    /// Chegou um quadro do par (bytes de `ir_proto::Frame`).
    Frame(Vec<u8>),
    /// O enlace caiu.
    LinkDown(&'static str),
    /// Um erro de rede que não derruba a tarefa.
    Error(String),
}
