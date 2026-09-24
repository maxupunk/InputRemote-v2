//! A parada do serviço: sair sem deixar nada preso.
//!
//! Parar é uma queda como outra qualquer, e vale a regra de toda queda: soltar tudo antes de
//! qualquer outra coisa. A diferença é que, depois, o processo some. A despedida ao par e o pedido
//! ao agente precisam sair enquanto ainda há quem os leve.

use std::time::Duration;

use ir_ipc::ComandoDoAgente;
use ir_session::{LinkDown, Phase};
use tracing::info;

use super::Daemon;

/// Quanto se espera, ao parar, para a despedida ao par e o pedido ao agente saírem.
const PRAZO_DE_DESPEDIDA: Duration = Duration::from_millis(300);

impl Daemon {
    /// O serviço vai parar: solta tudo, avisa o par e dispensa o agente, antes de o processo sumir.
    ///
    /// Antes o processo simplesmente acabava. O par só percebia pelo próprio prazo de queda, e o
    /// agente só saía quando o sistema fechava o canal dele — depois de o serviço já constar como
    /// parado, justamente quando um instalador tenta trocar o arquivo do agente.
    pub(super) async fn encerrar(&mut self) {
        info!("serviço parando: soltando tudo, avisando o par e dispensando o agente");
        if self.session.phase() != Phase::Offline {
            let agora = self.now();
            self.session
                .stop(agora, LinkDown::ServiceStopping, &mut self.out);
            self.apply_commands();
        }
        let _ = self.agente.send(ComandoDoAgente::Encerrar);
        // Um instante para a despedida e o pedido ao agente saírem pelos canais antes de a
        // runtime ser desmontada e levar as tarefas de envio junto.
        tokio::time::sleep(PRAZO_DE_DESPEDIDA).await;
    }
}

#[cfg(test)]
mod tests {
    use ir_proto::carrier::Carrier;
    use ir_session::Input;
    use tokio::sync::broadcast;

    use super::*;
    use crate::actor::bancada::Bancada;

    /// Se, entre tudo que o serviço mandou ao agente, está o pedido para ele sair.
    fn agente_dispensado(receptor: &mut broadcast::Receiver<ComandoDoAgente>) -> bool {
        let mut dispensado = false;
        while let Ok(comando) = receptor.try_recv() {
            dispensado |= matches!(comando, ComandoDoAgente::Encerrar);
        }
        dispensado
    }

    #[tokio::test]
    async fn ao_parar_a_sessao_cai_e_o_agente_e_dispensado() {
        let mut bancada = Bancada::nova();
        bancada.daemon.drive(Input::CarrierUp(Carrier::Udp));
        assert_ne!(
            bancada.daemon.session.phase(),
            Phase::Offline,
            "o teste precisa de uma sessão de pé para derrubar"
        );

        bancada.daemon.encerrar().await;

        assert_eq!(bancada.daemon.session.phase(), Phase::Offline);
        assert!(
            agente_dispensado(&mut bancada.agente),
            "o agente não pode ficar vivo além do serviço"
        );
    }

    #[tokio::test]
    async fn parar_sem_sessao_ainda_dispensa_o_agente() {
        // O caso da máquina sem par: o agente sobe com o serviço mesmo assim, e precisa sair junto.
        let mut bancada = Bancada::nova();

        bancada.daemon.encerrar().await;

        assert!(agente_dispensado(&mut bancada.agente));
    }
}
