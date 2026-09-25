//! Por que a última sessão terminou, com a frase de cada motivo.

use serde::{Deserialize, Serialize};

/// Por que a última sessão terminou.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum MotivoDaQueda {
    /// O usuário mandou parar.
    PedidoPeloUsuario,
    /// O serviço do outro lado está parando.
    ServicoDoParParando,
    /// O outro computador foi suspenso.
    ParSuspenso,
    /// O outro computador parou de responder.
    ParNaoRespondeu,
    /// O meio de conexão falhou: rádio desligado, cabo removido, socket fechado.
    MeioFalhou,
    /// Erro de protocolo.
    ErroDeProtocolo,
    /// Troca de meio em andamento.
    TrocandoDeMeio,
    // Daqui para baixo, as que entraram depois — no fim, porque o `postcard` grava pelo índice.
    /// Esta máquina foi suspensa.
    EstaMaquinaSuspensa,
    /// O outro computador recomeçou a sessão: reiniciou, ou reconfigurou algo.
    ParRecomecou,
    /// Quem está no outro computador pausou o compartilhamento.
    ParPausou,
}

impl MotivoDaQueda {
    /// A frase que a interface mostra.
    ///
    /// Cada uma diz **o que aconteceu**, não "erro". No v1, toda queda parecia igual, e
    /// diagnosticar era impossível ([00, §6](../../../docs/00-licoes-do-v1.md)).
    #[must_use]
    pub const fn frase(self) -> &'static str {
        match self {
            Self::PedidoPeloUsuario => "Você pausou a conexão",
            Self::ServicoDoParParando => "O serviço do outro computador está parando",
            Self::ParSuspenso => "O outro computador foi suspenso",
            Self::ParNaoRespondeu => "O outro computador parou de responder",
            Self::MeioFalhou => "O meio de conexão falhou",
            Self::ErroDeProtocolo => "Erro de protocolo entre as duas versões",
            Self::TrocandoDeMeio => "Trocando de meio de conexão",
            Self::EstaMaquinaSuspensa => "Este computador foi suspenso",
            Self::ParRecomecou => "O outro computador recomeçou a conexão",
            Self::ParPausou => "Pausado no outro computador",
        }
    }

    /// O peso desta queda.
    ///
    /// Sem ramo curinga de propósito: um motivo novo não compila até alguém decidir o peso dele.
    #[must_use]
    pub const fn gravidade(self) -> Gravidade {
        match self {
            Self::PedidoPeloUsuario | Self::ParPausou => Gravidade::Pedida,
            Self::ServicoDoParParando
            | Self::ParSuspenso
            | Self::TrocandoDeMeio
            | Self::EstaMaquinaSuspensa
            | Self::ParRecomecou => Gravidade::Esperada,
            Self::ParNaoRespondeu | Self::MeioFalhou | Self::ErroDeProtocolo => Gravidade::Falha,
        }
    }
}

/// O peso de uma queda: quem a provocou, e se ela pede atenção.
///
/// Uma decisão só, para a cor da tela e para o aviso fora da janela. Eram duas listas separadas, e
/// um par que se suspendia aparecia como normal na janela e como "Conexão perdida" na notificação.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Gravidade {
    /// Alguém escolheu: quem está aqui ou quem está lá. A pessoa já sabe.
    Pedida,
    /// Comportamento normal do produto ou do sistema — suspensão, troca de meio, um serviço
    /// reiniciando —, que se resolve sozinho.
    Esperada,
    /// Algo quebrou: é esta que a pessoa precisa ver, e é por isso que só ela alarma.
    Falha,
}
