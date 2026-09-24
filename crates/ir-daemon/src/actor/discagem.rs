//! O pedido de pareamento que saiu desta máquina: do clique em "Parear" até o código chegar — ou não.
//!
//! O pareamento propriamente dito ([`super::pareamento`]) só começa quando há código. Antes disso
//! havia um vão: o pedido saía, o outro computador não estava lá, e nada voltava — a janela dizia
//! "Aguardando o outro computador" para sempre, porque o prazo de dois minutos só existia depois
//! do código. Este módulo cobre o vão, com fim garantido e motivo.

use std::time::{Duration, Instant};

use ir_ipc::{Aviso, Falha};
use ir_proto::carrier::Carrier;
use tracing::warn;

use super::Daemon;

/// Quanto se espera o outro lado responder a um pedido de pareamento.
///
/// Um pouco mais que o reenvio do transporte (12 s no `ir-net`), para o erro dele chegar primeiro;
/// o prazo daqui é a rede de segurança para um transporte que não diz nada.
const PRAZO_DA_DISCAGEM: Duration = Duration::from_secs(20);

/// Um pedido de pareamento em curso.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Discagem {
    /// Por onde se discou: é o erro **deste** portador que encerra a espera.
    portador: Carrier,
    /// Quando o pedido saiu.
    pub(super) desde: Instant,
}

impl Daemon {
    /// O pedido de pareamento saiu, por este portador.
    pub(super) fn discagem_comecou(&mut self, portador: Carrier) {
        self.discagem = Some(Discagem {
            portador,
            desde: Instant::now(),
        });
    }

    /// O código chegou: daqui em diante quem conta o tempo é o pareamento.
    pub(super) const fn discagem_atendida(&mut self) {
        self.discagem = None;
    }

    /// O transporte relatou um erro. Se é do portador por onde se discou, o pedido não vai ser
    /// atendido.
    pub(super) fn discagem_com_erro(&mut self, portador: Carrier) {
        if self.discagem.is_some_and(|d| d.portador == portador) {
            self.falhar_discagem();
        }
    }

    /// Desiste do pedido se ele passou do prazo sem resposta nenhuma.
    pub(super) fn vencer_discagem_se_preciso(&mut self) {
        if self
            .discagem
            .is_some_and(|d| d.desde.elapsed() >= PRAZO_DA_DISCAGEM)
        {
            self.falhar_discagem();
        }
    }

    fn falhar_discagem(&mut self) {
        self.discagem = None;
        warn!("o pedido de pareamento não teve resposta do outro computador");
        let _ = self
            .avisos
            .send(Aviso::PareamentoFalhou(Falha::ParNaoRespondeu));
    }
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use ir_crypto::PublicKey;
    use ir_ipc::Pedido;
    use ir_transporte::Fato;

    use super::*;
    use crate::actor::bancada::Bancada;

    fn avisos_de(receptor: &mut tokio::sync::broadcast::Receiver<Aviso>) -> Vec<Aviso> {
        let mut todos = Vec::new();
        while let Ok(aviso) = receptor.try_recv() {
            todos.push(aviso);
        }
        todos
    }

    fn pedir_pareamento(bancada: &mut Bancada) {
        let _ = bancada.daemon.tratar(
            Pedido::IniciarPareamento {
                candidato: "10.0.0.9:52525".to_owned(),
            },
            ir_transferencia::Leitor::Proprio,
        );
    }

    #[test]
    fn o_pedido_que_o_transporte_nao_conseguiu_entregar_vira_falha_com_motivo() {
        let mut bancada = Bancada::nova();
        let mut avisos = bancada.daemon.avisos.subscribe();
        pedir_pareamento(&mut bancada);
        bancada.daemon.on_fato_do_transporte(Fato::Erro {
            portador: Carrier::Udp,
            mensagem: "o par não respondeu a tempo".to_owned(),
        });
        let recebidos = avisos_de(&mut avisos);
        assert!(
            recebidos.contains(&Aviso::PareamentoFalhou(Falha::ParNaoRespondeu)),
            "{recebidos:?}"
        );
    }

    #[test]
    fn o_erro_de_outro_portador_nao_encerra_o_pedido() {
        let mut bancada = Bancada::nova();
        let mut avisos = bancada.daemon.avisos.subscribe();
        pedir_pareamento(&mut bancada);
        bancada.daemon.on_fato_do_transporte(Fato::Erro {
            portador: Carrier::Rfcomm,
            mensagem: "rádio ocupado".to_owned(),
        });
        assert!(avisos_de(&mut avisos).is_empty());
        assert!(bancada.daemon.discagem.is_some());
    }

    #[test]
    fn o_codigo_que_chega_encerra_a_espera_sem_falha() {
        let mut bancada = Bancada::nova();
        let mut avisos = bancada.daemon.avisos.subscribe();
        pedir_pareamento(&mut bancada);
        bancada
            .daemon
            .on_fato_do_transporte(Fato::CodigoDePareamento {
                portador: Carrier::Udp,
                digitos: [1, 2, 3, 4, 5, 6],
                chave_do_par: PublicKey([4; 32]),
                de: ir_transporte::Endereco::ler("10.0.0.9:52525").expect("endereço"),
            });
        bancada.daemon.vencer_discagem_se_preciso();
        assert!(bancada.daemon.discagem.is_none());
        let recebidos = avisos_de(&mut avisos);
        assert!(
            !recebidos
                .iter()
                .any(|aviso| matches!(aviso, Aviso::PareamentoFalhou(_))),
            "{recebidos:?}"
        );
    }

    #[test]
    fn sem_resposta_nenhuma_o_prazo_encerra_o_pedido() {
        let mut bancada = Bancada::nova();
        let mut avisos = bancada.daemon.avisos.subscribe();
        pedir_pareamento(&mut bancada);
        if let Some(discagem) = bancada.daemon.discagem.as_mut() {
            discagem.desde = Instant::now()
                .checked_sub(PRAZO_DA_DISCAGEM + Duration::from_secs(1))
                .expect("instante no passado");
        }
        bancada.daemon.vencer_discagem_se_preciso();
        assert!(avisos_de(&mut avisos).contains(&Aviso::PareamentoFalhou(Falha::ParNaoRespondeu)));
    }

    #[test]
    fn a_janela_que_chega_depois_do_codigo_tambem_o_recebe() {
        // O defeito #1 do log 21: o código ia por aviso uma vez só, e a janela aberta depois — ou
        // tirada da bandeja — ficava sem nada para comparar.
        let mut bancada = Bancada::nova();
        bancada
            .daemon
            .on_pairing_code([9, 8, 7, 6, 5, 4], PublicKey([4; 32]));
        let mut atrasada = bancada.daemon.avisos.subscribe();
        let _ = bancada
            .daemon
            .tratar(Pedido::Acompanhar, ir_transferencia::Leitor::Proprio);
        assert!(
            avisos_de(&mut atrasada).contains(&Aviso::CodigoDePareamento {
                digitos: [9, 8, 7, 6, 5, 4]
            })
        );

        // Depois de conferido, não há mais o que comparar: nada é recontado.
        bancada.daemon.confirmar(true);
        let _ = avisos_de(&mut atrasada);
        let _ = bancada
            .daemon
            .tratar(Pedido::Acompanhar, ir_transferencia::Leitor::Proprio);
        assert!(
            !avisos_de(&mut atrasada)
                .iter()
                .any(|aviso| matches!(aviso, Aviso::CodigoDePareamento { .. }))
        );
    }
}
