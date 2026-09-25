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

    /// O rádio ainda tem uma conexão anterior com este par.
    ///
    /// O enlace de baixo nível sobrevive ao processo: um programa encerrado de repente deixa a
    /// sessão RFCOMM meio aberta, e a conexão seguinte para o mesmo canal é recusada como ocupada
    /// até o sistema expirar o enlace sozinho. Distinto de [`Self::SemResposta`]: ali não há
    /// ninguém atendendo, e aqui há **conexão demais**, não de menos (log 28).
    #[error("o rádio ainda está ocupado com uma conexão anterior a {0}")]
    Ocupado(String),

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

/// Por que uma conexão ao par falhou, em termos que não dependem do sistema operacional.
///
/// Cada sistema só diz qual código cru é qual situação; o que o usuário lê sai de um lugar só,
/// [`para_erro`]. Antes cada backend escolhia a própria mensagem, e as duas já tinham divergido: o
/// mesmo "o rádio não alcança o par" era "não está pareado" no Linux e "não atendeu" no Windows.
#[derive(Debug)]
pub enum FalhaDeConexao {
    /// O rádio alcançou o par, e ninguém atende no canal do produto.
    Recusada,
    /// O rádio não alcançou o par, ou ele não respondeu no prazo: longe, desligado, sem rádio.
    SemAlcance,
    /// O sistema recusa conectar porque não tem vínculo gravado com este endereço.
    NaoPareado,
    /// O sistema ainda segura uma sessão anterior com este par, no mesmo canal (log 28).
    Ocupado,
    /// O rádio desta máquina caiu no meio da tentativa.
    RadioCaiu,
    /// Outra coisa, que não se sabe explicar melhor que o próprio erro.
    Outra(std::io::Error),
}

/// A mensagem de uma falha de conexão ao par `alvo`.
///
/// O ADR-0005 exige distinguir "não pareado no sistema" de "pareado, mas o serviço não responde" —
/// e dizer qual é. Não alcançar o par cai no segundo: do lado de cá não há como saber se ele está
/// longe ou com o serviço parado, e a instrução ("veja se o outro está ligado e com o InputRemote
/// rodando") serve aos dois.
#[must_use]
pub fn para_erro(falha: FalhaDeConexao, alvo: crate::BdAddr) -> BtError {
    match falha {
        FalhaDeConexao::Recusada | FalhaDeConexao::SemAlcance => BtError::SemResposta,
        FalhaDeConexao::NaoPareado => BtError::NaoPareado(alvo.to_string()),
        FalhaDeConexao::Ocupado => BtError::Ocupado(alvo.to_string()),
        FalhaDeConexao::RadioCaiu => BtError::SemRadio("o rádio Bluetooth caiu".to_owned()),
        FalhaDeConexao::Outra(erro) => BtError::Io(erro),
    }
}

impl From<ir_crypto::enlace::Excesso> for BtError {
    fn from(excesso: ir_crypto::enlace::Excesso) -> Self {
        Self::GrandeDemais {
            tamanho: excesso.tamanho,
            limite: excesso.limite,
        }
    }
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
            Self::Ocupado(_) => Some(
                "Uma conexão anterior com esse computador ainda não terminou. Espere alguns \
                 segundos, ou desconecte-o pelas configurações de Bluetooth do sistema.",
            ),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn as_causas_que_o_usuario_resolve_tem_instrucao() {
        // O ADR-0005 exige distinguir "não pareado" de "pareado, mas sem resposta". Distinguir
        // só serve se cada uma disser o que fazer a respeito.
        let com_instrucao = [
            BtError::SemRadio("hci0 bloqueado".to_owned()),
            BtError::NaoPareado("AA:BB:CC:DD:EE:FF".to_owned()),
            BtError::SemResposta,
            BtError::Ocupado("AA:BB:CC:DD:EE:FF".to_owned()),
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
    fn cada_falha_de_conexao_vira_a_mensagem_da_sua_situacao() {
        let alvo = crate::BdAddr([0xAC, 0x50, 0xDE, 0x47, 0xEB, 0x28]);
        assert!(matches!(
            para_erro(FalhaDeConexao::Recusada, alvo),
            BtError::SemResposta
        ));
        assert!(matches!(
            para_erro(FalhaDeConexao::SemAlcance, alvo),
            BtError::SemResposta
        ));
        assert!(matches!(
            para_erro(FalhaDeConexao::NaoPareado, alvo),
            BtError::NaoPareado(ref quem) if quem == "AC:50:DE:47:EB:28"
        ));
        assert!(matches!(
            para_erro(FalhaDeConexao::Ocupado, alvo),
            BtError::Ocupado(ref quem) if quem == "AC:50:DE:47:EB:28"
        ));
        assert!(matches!(
            para_erro(FalhaDeConexao::RadioCaiu, alvo),
            BtError::SemRadio(_)
        ));
        let cru = std::io::Error::other("qualquer");
        assert!(matches!(
            para_erro(FalhaDeConexao::Outra(cru), alvo),
            BtError::Io(ref erro) if erro.to_string() == "qualquer"
        ));
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
