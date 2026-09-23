//! O que a janela mostra para responder "por que não está bom?": a latência e a última queda.
//!
//! [01, §5](../../../docs/01-visao-e-escopo.md) exige a latência mediana e o p99 da última janela
//! de 10 s, e a razão da última queda. A sessão já media a volta de cada batida e sabia por que
//! caía; o serviço só registrava em `debug` e mandava `None` para a tela.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use ir_ipc::{Latencia, MotivoDaQueda};
use ir_proto::message::DisconnectReason;
use ir_session::LinkDown;

/// A janela da latência publicada.
pub const JANELA: Duration = Duration::from_secs(10);

/// Quantas amostras cabem, no máximo — a sessão mede uma por batida, e isto é folga larga.
const CAPACIDADE: usize = 256;

/// As voltas medidas na última janela.
#[derive(Debug, Default)]
pub struct Voltas {
    amostras: VecDeque<(Instant, u32)>,
}

impl Voltas {
    /// Anota uma volta.
    pub fn anotar(&mut self, agora: Instant, milissegundos: u32) {
        self.esquecer_velhas(agora);
        if self.amostras.len() >= CAPACIDADE {
            self.amostras.pop_front();
        }
        self.amostras.push_back((agora, milissegundos));
    }

    /// Esquece tudo: numa sessão nova, as voltas da anterior não dizem nada.
    pub fn esquecer(&mut self) {
        self.amostras.clear();
    }

    /// A mediana e o p99 da janela, se houver amostra.
    #[must_use]
    pub fn latencia(&self, agora: Instant) -> Option<Latencia> {
        let mut valores: Vec<u32> = self
            .amostras
            .iter()
            .filter(|(quando, _)| agora.saturating_duration_since(*quando) <= JANELA)
            .map(|(_, ms)| *ms)
            .collect();
        if valores.is_empty() {
            return None;
        }
        valores.sort_unstable();
        let posicao = |fracao: f64| {
            // Posição pelo método do vizinho mais próximo: com poucas amostras, o p99 é o pior caso.
            let total = valores.len();
            #[allow(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                clippy::cast_precision_loss
            )]
            let indice = ((fracao * total as f64).ceil() as usize).clamp(1, total) - 1;
            valores.get(indice).copied().unwrap_or(0)
        };
        Some(Latencia {
            mediana_ms: posicao(0.5),
            p99_ms: posicao(0.99),
            amostras: u32::try_from(valores.len()).unwrap_or(u32::MAX),
        })
    }

    fn esquecer_velhas(&mut self, agora: Instant) {
        while self
            .amostras
            .front()
            .is_some_and(|(quando, _)| agora.saturating_duration_since(*quando) > JANELA)
        {
            self.amostras.pop_front();
        }
    }
}

/// A razão de uma queda, no vocabulário da janela.
///
/// Cada uma diz o que aconteceu de verdade. A pausa pedida do outro lado não é "você encerrou", e
/// a suspensão desta máquina não é "o outro computador foi suspenso".
#[allow(clippy::match_same_arms)] // um braço por motivo, mesmo quando a frase coincide
#[must_use]
pub fn motivo_da_queda(motivo: LinkDown) -> MotivoDaQueda {
    match motivo {
        LinkDown::UserStopped => MotivoDaQueda::PedidoPeloUsuario,
        LinkDown::Suspending => MotivoDaQueda::EstaMaquinaSuspensa,
        LinkDown::Timeout | LinkDown::PeerClosed(DisconnectReason::Timeout) => {
            MotivoDaQueda::ParNaoRespondeu
        }
        LinkDown::TransportFailed => MotivoDaQueda::MeioFalhou,
        LinkDown::ServiceStopping => MotivoDaQueda::PedidoPeloUsuario,
        LinkDown::PeerRestarted | LinkDown::PeerClosed(DisconnectReason::Reconfiguring) => {
            MotivoDaQueda::ParRecomecou
        }
        LinkDown::PeerClosed(DisconnectReason::UserRequested) => MotivoDaQueda::ParPausou,
        LinkDown::PeerClosed(DisconnectReason::ServiceStopping) => {
            MotivoDaQueda::ServicoDoParParando
        }
        LinkDown::PeerClosed(DisconnectReason::Suspending) => MotivoDaQueda::ParSuspenso,
        LinkDown::PeerClosed(DisconnectReason::SwitchingCarrier) => MotivoDaQueda::TrocandoDeMeio,
        LinkDown::PeerClosed(_) => MotivoDaQueda::ErroDeProtocolo,
        // O enum é não exaustivo: um motivo novo da sessão, até ter frase própria, é do meio.
        _ => MotivoDaQueda::MeioFalhou,
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn sem_amostra_nao_ha_latencia() {
        assert_eq!(Voltas::default().latencia(Instant::now()), None);
    }

    #[test]
    fn a_mediana_e_o_pior_caso_saem_da_janela() {
        let inicio = Instant::now();
        let mut voltas = Voltas::default();
        for (segundo, ms) in [3, 4, 5, 4, 120, 3, 4, 5, 4, 3].into_iter().enumerate() {
            voltas.anotar(inicio + Duration::from_millis(900 * segundo as u64), ms);
        }
        let medida = voltas.latencia(inicio + Duration::from_secs(9)).unwrap();
        assert_eq!(medida.mediana_ms, 4);
        assert_eq!(
            medida.p99_ms, 120,
            "o pior caso aparece, que é o que o usuário sente"
        );
        assert_eq!(medida.amostras, 10);
    }

    #[test]
    fn o_que_saiu_da_janela_nao_conta() {
        let inicio = Instant::now();
        let mut voltas = Voltas::default();
        voltas.anotar(inicio, 300);
        voltas.anotar(inicio + Duration::from_secs(15), 5);
        let medida = voltas.latencia(inicio + Duration::from_secs(15)).unwrap();
        assert_eq!(
            (medida.mediana_ms, medida.p99_ms, medida.amostras),
            (5, 5, 1)
        );
    }

    #[test]
    fn cada_queda_tem_a_frase_do_que_aconteceu() {
        assert_eq!(
            motivo_da_queda(LinkDown::PeerClosed(DisconnectReason::UserRequested)),
            MotivoDaQueda::ParPausou,
            "o outro lado pausou; não foi você"
        );
        assert_eq!(
            motivo_da_queda(LinkDown::Suspending),
            MotivoDaQueda::EstaMaquinaSuspensa
        );
        assert_eq!(
            motivo_da_queda(LinkDown::PeerClosed(DisconnectReason::Suspending)),
            MotivoDaQueda::ParSuspenso
        );
        assert_eq!(
            motivo_da_queda(LinkDown::Timeout),
            MotivoDaQueda::ParNaoRespondeu
        );
    }
}
