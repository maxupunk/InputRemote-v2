//! O tipo de erro do crate.
//!
//! Como em `ir-proto`, nenhuma variante carrega texto claro nem material de chave: um erro que
//! aparece no log não pode revelar o que estava sendo cifrado nem com o quê
//! ([04, §7](../../../docs/04-seguranca.md)).

/// Resultado das operações deste crate.
pub type Result<T> = core::result::Result<T, CryptoError>;

/// O que pode dar errado na criptografia e no pareamento.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum CryptoError {
    /// O handshake do Noise falhou (mensagem adulterada, fora de ordem, ou chave incompatível).
    #[error("o handshake seguro falhou")]
    Handshake,

    /// Tentou-se escrever ou ler uma mensagem de handshake fora da vez.
    #[error("mensagem de handshake fora de ordem")]
    OutOfTurn,

    /// O handshake ainda não terminou; não há transporte para usar.
    #[error("o handshake ainda não terminou")]
    NotFinished,

    /// A decifragem falhou: a tag não confere. Bytes adulterados, ou de outra sessão.
    #[error("a mensagem cifrada não pôde ser aberta")]
    Open,

    /// O contador já foi visto, ou é antigo demais para a janela de repetição.
    ///
    /// É a defesa contra reenvio: um `KeyDown` gravado do rádio e reproduzido cai aqui
    /// ([04, §2](../../../docs/04-seguranca.md)).
    #[error("mensagem repetida ou fora da janela")]
    Replay,

    /// A chave estática apresentada não é a que estava fixada para este par.
    ///
    /// Recusa, nunca pergunta ao usuário ([04, §3.3](../../../docs/04-seguranca.md)).
    #[error("a identidade do par não confere com a fixada")]
    WrongPeer,

    /// Um material de chave tem tamanho errado.
    #[error("material de chave com tamanho inválido")]
    BadKeyLength,
}
