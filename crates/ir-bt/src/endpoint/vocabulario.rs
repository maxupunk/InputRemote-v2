//! O que o serviço manda ao endpoint, e o que o endpoint conta de volta.
//!
//! Fica num arquivo próprio porque é a parte **pública**: é isto que o `ir-daemon` consome, e a
//! máquina de estados ao lado é detalhe interno. Separar deixa a fronteira legível sem precisar
//! ler o laço.
//!
//! A forma é deliberadamente a mesma do `NetCommand`/`NetEvent` do `ir-net`. Dois portadores com
//! o mesmo formato de comando e evento permitem ao serviço tratá-los pela mesma porta, em vez de
//! ter um caminho de código por portador — que foi como o v1 acabou com três políticas de
//! degradação diferentes ([00, §6](../../../../docs/00-licoes-do-v1.md)).

use ir_crypto::PublicKey;
use tokio::time::Instant;

use crate::addr::BdAddr;
use crate::handshake::ConnectMode;

/// O que o serviço manda ao endpoint.
#[derive(Debug)]
#[non_exhaustive]
pub enum BtCommand {
    /// Comece a conectar, como iniciador.
    Connect {
        /// O endereço do rádio do par.
        peer: BdAddr,
        /// Parear do zero ou reconectar com a chave fixada.
        mode: ConnectMode,
    },
    /// Mande este quadro (bytes já codificados de `ir_proto::Frame`) ao par.
    ///
    /// Carrega o instante em que entrou na fila porque o rádio pode travar: sob interferência o
    /// RFCOMM para de escoar, a fila cresce, e quando ele volta despejaria segundos de quadros
    /// velhos na frente dos novos. O endpoint descarta o que passou de
    /// [`VELHO_DEMAIS`](super::VELHO_DEMAIS) **antes** de cifrar — depois não dá, porque o contador
    /// do enlace é implícito ([`link`](crate::link)). Use [`BtCommand::quadro`].
    SendFrame {
        /// Os bytes do quadro.
        bytes: Vec<u8>,
        /// Quando o quadro entrou na fila.
        queued_at: Instant,
    },
    /// O usuário respondeu à comparação de códigos.
    ConfirmPairing(bool),
    /// Se um pedido de pareamento que chega de fora é atendido.
    ///
    /// O serviço desliga isto quando já há par e a janela de pareamento não está aberta: sem a
    /// chave, qualquer um por perto punha um código na tela e ocupava o endpoint (log 45).
    AcceptPairing(bool),
    /// Encerre o enlace atual.
    Disconnect,
    /// Encerre a tarefa.
    Shutdown,
}

impl BtCommand {
    /// Um quadro para mandar, carimbado com o instante de agora.
    #[must_use]
    pub fn quadro(bytes: Vec<u8>) -> Self {
        Self::SendFrame {
            bytes,
            queued_at: Instant::now(),
        }
    }
}

/// O que o endpoint conta ao serviço.
#[derive(Debug)]
#[non_exhaustive]
pub enum BtEvent {
    /// O handshake de pareamento terminou; aqui está o código para o usuário comparar.
    PairingCode {
        /// Os seis dígitos.
        code: [u8; 6],
        /// A chave estática que o par apresentou, para gravar após a confirmação.
        peer_static: PublicKey,
        /// O endereço do par.
        peer: BdAddr,
    },
    /// O enlace está pronto: pareamento confirmado dos dois lados, ou reconexão fixada.
    Established {
        /// A chave estática do par.
        peer_static: PublicKey,
        /// O endereço do par.
        peer: BdAddr,
    },
    /// Chegou um quadro do par (bytes de `ir_proto::Frame`).
    Frame(Vec<u8>),
    /// O enlace caiu.
    LinkDown(&'static str),
    /// Um erro que não derruba a tarefa.
    ///
    /// Traz a instrução ao usuário quando existe uma — "pareie os dois computadores nas
    /// configurações do sistema" é acionável; "mensagem malformada" não é, e inventar conselho
    /// para falha interna treina o usuário a ignorar a mensagem.
    Error(String),
    /// O rádio parou de escutar: o adaptador foi desligado ou removido.
    ///
    /// O endpoint termina depois deste evento. Antes ele ficava esperando uma ligação que nunca
    /// viria — no Windows — ou girava em erro de `accept` — no Linux —, e o Bluetooth só voltava
    /// reiniciando o serviço.
    RadioLost(String),
}
