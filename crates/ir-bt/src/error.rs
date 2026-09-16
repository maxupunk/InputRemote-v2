//! O tipo de erro do crate.
//!
//! As variantes existem para o diagnóstico que o ADR-0005 exige em letras: *"distinguir 'não
//! pareado no sistema' de 'pareado, mas o serviço não responde' — e dizer qual é"*. Um
//! `io::Error` cru não faz essa distinção, e é ela que o usuário precisa para saber se o
//! problema é dele ou nosso.

/// Resultado das operações de Bluetooth.
pub type Result<T> = core::result::Result<T, BtError>;

/// O que pode dar errado no transporte Bluetooth.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum BtError {
    /// Não há rádio Bluetooth utilizável nesta máquina.
    ///
    /// Sem adaptador, desligado, ou bloqueado por software. É o caso em que o produto **não**
    /// deve insistir: ele degrada para a rede e diz o motivo.
    #[error("não há rádio Bluetooth disponível: {0}")]
    SemRadio(String),

    /// O par não está pareado neste sistema.
    ///
    /// O pareamento do **sistema** é do usuário, pelas configurações do próprio sistema
    /// operacional ([ADR-0005](../../../docs/adr/0005-bluetooth-rfcomm-winsock.md), decisão B).
    /// O produto detecta e explica; não conduz.
    #[error("o computador {0} não está pareado no sistema")]
    NaoPareado(String),

    /// Pareado, mas ninguém atendeu o canal RFCOMM do produto.
    ///
    /// Distinto de [`Self::NaoPareado`] de propósito: aqui o rádio alcança o par, e o que falta
    /// é o serviço do outro lado estar no ar.
    #[error("o par não atendeu no canal do InputRemote")]
    SemResposta,

    /// Falha de E/S no socket RFCOMM.
    #[error("erro de socket Bluetooth: {0}")]
    Io(#[from] std::io::Error),

    /// A criptografia recusou (handshake inválido, tag que não confere, repetição).
    #[error("erro de criptografia: {0}")]
    Crypto(#[from] ir_crypto::CryptoError),

    /// O par não respondeu a tempo durante o handshake.
    #[error("o par não respondeu a tempo")]
    HandshakeTimeout,

    /// Os bytes recebidos não formam uma mensagem válida para a fase atual.
    #[error("mensagem malformada")]
    Malformed,

    /// A mensagem anunciada passa do teto do portador.
    ///
    /// Conferido **antes** de alocar: o serviço roda privilegiado e recebe bytes de um rádio que
    /// qualquer um alcança ([04, §1](../../../docs/04-seguranca.md)).
    #[error("mensagem de {tamanho} B passa do teto de {limite} B do RFCOMM")]
    GrandeDemais {
        /// O tamanho anunciado.
        tamanho: usize,
        /// O teto do portador.
        limite: usize,
    },

    /// O par recusou o pareamento (códigos diferentes).
    #[error("o pareamento foi recusado pelo par")]
    PairRejected,

    /// A chave estática apresentada não é a fixada para este par.
    #[error("a identidade do par não confere com a fixada")]
    WrongPeer,
}

impl BtError {
    /// O que o usuário deve fazer, quando há o que fazer.
    ///
    /// Só as causas que uma pessoa resolve têm instrução. Inventar conselho para falha interna
    /// treina o usuário a ignorar a mensagem.
    #[must_use]
    pub const fn o_que_fazer(&self) -> Option<&'static str> {
        match self {
            Self::SemRadio(_) => Some(
                "Ligue o Bluetooth nas configurações do sistema. Sem rádio, a conexão usa a \
                 rede local.",
            ),
            Self::NaoPareado(_) => Some(
                "Pareie os dois computadores pelas configurações de Bluetooth do sistema, uma \
                 vez. O código de seis dígitos do InputRemote é outro, e vem depois.",
            ),
            Self::SemResposta => Some(
                "Verifique se o InputRemote está em execução no outro computador e se o \
                 Bluetooth dele está ligado.",
            ),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn as_tres_causas_que_o_usuario_resolve_tem_instrucao() {
        // O ADR-0005 exige distinguir "não pareado" de "pareado, mas sem resposta". Distinguir
        // só serve se cada uma disser o que fazer a respeito.
        let com_instrucao = [
            BtError::SemRadio("hci0 bloqueado".to_owned()),
            BtError::NaoPareado("AA:BB:CC:DD:EE:FF".to_owned()),
            BtError::SemResposta,
        ];
        for erro in com_instrucao {
            let instrucao = erro.o_que_fazer().expect("precisa instruir");
            assert!(instrucao.len() > 20, "{erro}: `{instrucao}` não instrui");
        }
    }

    #[test]
    fn falha_interna_nao_inventa_conselho() {
        assert!(BtError::Malformed.o_que_fazer().is_none());
        assert!(BtError::HandshakeTimeout.o_que_fazer().is_none());
    }

    #[test]
    fn nao_pareado_e_sem_resposta_sao_mensagens_diferentes() {
        // São os dois lados da pergunta "por que não conecta?", e confundi-los manda o usuário
        // mexer no lugar errado.
        let nao_pareado = BtError::NaoPareado("AA:BB:CC:DD:EE:FF".to_owned()).to_string();
        let sem_resposta = BtError::SemResposta.to_string();
        assert_ne!(nao_pareado, sem_resposta);
        assert!(nao_pareado.contains("pareado"));
    }
}
