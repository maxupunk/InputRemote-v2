//! O tipo de erro do crate.

/// Resultado das operações de rede.
pub type Result<T> = core::result::Result<T, NetError>;

/// O que pode dar errado na rede.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum NetError {
    /// Falha de E/S no socket.
    #[error("erro de socket: {0}")]
    Io(#[from] std::io::Error),

    /// A criptografia recusou (handshake inválido, tag que não confere, repetição).
    #[error("erro de criptografia: {0}")]
    Crypto(#[from] ir_crypto::CryptoError),

    /// O par não respondeu a tempo durante o handshake.
    #[error("o par não respondeu a tempo")]
    HandshakeTimeout,

    /// O datagrama recebido não tem o formato esperado para a fase atual.
    #[error("datagrama malformado")]
    Malformed,

    /// O par recusou o pareamento (códigos diferentes).
    #[error("o pareamento foi recusado pelo par")]
    PairRejected,

    /// A chave estática apresentada não é a fixada para este par.
    #[error("a identidade do par não confere com a fixada")]
    WrongPeer,

    /// O par anunciou um corpo maior que o teto do portador.
    ///
    /// Só existe nos portadores de *stream*, onde há prefixo de tamanho para mentir. Conferido
    /// **antes** de reservar memória: estes bytes chegam num processo privilegiado
    /// ([04, §1](../../../docs/04-seguranca.md)).
    #[error("o par anunciou {size} B, acima do teto de {limit} B")]
    TooLarge {
        /// O tamanho anunciado.
        size: usize,
        /// O teto do portador.
        limit: usize,
    },

    /// O par fechou o *stream*.
    ///
    /// Não é erro de socket, e a distinção importa: "o par encerrou" é o que o usuário precisa
    /// ouvir, e não "erro de E/S".
    #[error("o par encerrou a conexão")]
    Closed,
}
