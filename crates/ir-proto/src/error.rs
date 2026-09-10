//! O único tipo de erro deste crate.
//!
//! Regra de `docs/09-padroes-de-codigo.md` §5: biblioteca usa `thiserror` com enum
//! específico. E regra de segurança de `docs/04-seguranca.md` §7: **nenhuma variante
//! carrega conteúdo decodificado**. Um erro descreve *o que* estava errado na estrutura,
//! nunca *o que* os bytes diziam — porque esses bytes podem ser uma senha, e o erro vai
//! para o log.

use core::fmt;

/// Resultado das operações deste crate.
pub type Result<T> = core::result::Result<T, ProtoError>;

/// Falha ao codificar, decodificar ou validar uma mensagem do protocolo.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum ProtoError {
    /// Os bytes não formam uma mensagem válida.
    ///
    /// Não diz qual byte nem qual valor: um decodificador que descreve a entrada inválida
    /// em detalhe é um oráculo para quem está sondando o serviço.
    #[error("bytes não formam uma mensagem válida do protocolo")]
    Malformed,

    /// A mensagem terminou antes do esperado.
    #[error("mensagem truncada")]
    Truncated,

    /// Sobraram bytes depois da mensagem.
    ///
    /// Tratado como erro, não ignorado. Um quadro com sobra significa que as duas pontas
    /// discordam sobre o formato, e agir sob discordância é digitar a coisa errada na
    /// máquina do outro.
    #[error("bytes sobrando após o fim da mensagem")]
    TrailingBytes,

    /// A mensagem passou do tamanho permitido para o seu canal ou portador.
    #[error("mensagem excede o tamanho máximo: {actual} B, máximo {limit} B")]
    TooLarge {
        /// Tamanho encontrado, em bytes.
        actual: usize,
        /// Tamanho máximo permitido, em bytes.
        limit: usize,
    },

    /// O discriminante de tipo de mensagem não é conhecido nesta versão.
    ///
    /// Derruba o enlace, não é ignorado — `docs/03-protocolo.md` §8.
    #[error("tipo de mensagem desconhecido: {discriminant}")]
    UnknownMessage {
        /// O discriminante recebido.
        discriminant: u8,
    },

    /// O identificador de canal não existe.
    #[error("canal desconhecido: {channel}")]
    UnknownChannel {
        /// O identificador recebido.
        channel: u8,
    },

    /// Esta mensagem não pode viajar por este canal.
    ///
    /// Existe para impedir, por exemplo, um bloco de arquivo chegar pelo canal do ponteiro.
    #[error("mensagem não permitida no canal {channel}")]
    WrongChannel {
        /// O canal em que a mensagem chegou.
        channel: ChannelName,
    },

    /// As duas pontas não têm uma versão de protocolo em comum.
    #[error("versões de protocolo incompatíveis: local {local}, remota {remote}")]
    IncompatibleVersion {
        /// Versão desta ponta.
        local: u16,
        /// Versão anunciada pela outra ponta.
        remote: u16,
    },

    /// Um campo de contagem anunciou mais itens do que o limite permite.
    ///
    /// Verificado **antes** de alocar. É a defesa contra um par remoto que anuncia um
    /// milhão de monitores para fazer o serviço `SYSTEM` esgotar memória.
    #[error("contagem de {what} acima do limite: {actual}, máximo {limit}")]
    CountTooLarge {
        /// O que estava sendo contado.
        what: &'static str,
        /// Contagem anunciada.
        actual: usize,
        /// Limite permitido.
        limit: usize,
    },
}

/// Nome estático de canal, para uso em mensagens de erro sem alocar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChannelName(pub &'static str);

impl fmt::Display for ChannelName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0)
    }
}

impl From<postcard::Error> for ProtoError {
    fn from(err: postcard::Error) -> Self {
        use postcard::Error as P;
        match err {
            P::DeserializeUnexpectedEnd => Self::Truncated,
            _ => Self::Malformed,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_messages_never_contain_decoded_payload() {
        // Não há como um teste provar isto para sempre, mas ele documenta a intenção e
        // falha se alguém acrescentar um campo de conteúdo a uma variante existente.
        let cases = [
            ProtoError::Malformed,
            ProtoError::Truncated,
            ProtoError::TrailingBytes,
            ProtoError::TooLarge {
                actual: 99,
                limit: 64,
            },
            ProtoError::UnknownMessage { discriminant: 0xFF },
            ProtoError::UnknownChannel { channel: 9 },
            ProtoError::WrongChannel {
                channel: ChannelName("ponteiro"),
            },
            ProtoError::IncompatibleVersion {
                local: 1,
                remote: 7,
            },
            ProtoError::CountTooLarge {
                what: "monitores",
                actual: 999,
                limit: 16,
            },
        ];
        for case in cases {
            let text = case.to_string();
            assert!(!text.is_empty());
            // Nenhuma variante carrega bytes ou String vinda do fio.
            assert!(!text.contains('\u{0}'));
        }
    }

    #[test]
    fn truncation_from_postcard_is_distinguished_from_malformed() {
        let err: ProtoError = postcard::Error::DeserializeUnexpectedEnd.into();
        assert_eq!(err, ProtoError::Truncated);
        let err: ProtoError = postcard::Error::SerdeDeCustom.into();
        assert_eq!(err, ProtoError::Malformed);
    }
}
