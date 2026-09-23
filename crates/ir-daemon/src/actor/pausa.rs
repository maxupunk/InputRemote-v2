//! Pausar e retomar o compartilhamento — de verdade.
//!
//! "Encerrar conexão" só derrubava os enlaces: o par não era avisado, e a reconexão dos dois lados
//! discava de novo em 3 s. O botão parecia não funcionar. Pausar agora é uma queda anunciada: a
//! sessão se despede com "pedido pelo usuário", ninguém disca, e um enlace que chegar é recusado.
//! O outro lado entende a despedida e também para de discar, mostrando que foi pausado lá.
//!
//! A pausa vive na memória do serviço: reiniciar o computador volta a compartilhar, que é o que se
//! espera de quem esqueceu de retomar.

use ir_ipc::{MotivoDaQueda, Pausa, Resposta};
use ir_session::{LinkDown, Phase};
use tracing::info;

use super::Daemon;

impl Daemon {
    /// Pausa aqui: despede-se do par e para de discar.
    pub(super) fn pausar(&mut self) -> Resposta {
        info!("compartilhamento pausado aqui");
        self.pausa = Some(Pausa::Aqui);
        if self.session.phase() != Phase::Offline {
            let agora = self.now();
            self.session
                .stop(agora, LinkDown::UserStopped, &mut self.out);
            self.apply_commands();
        }
        self.desconectar_todos();
        self.ultima_queda = Some(MotivoDaQueda::PedidoPeloUsuario);
        self.notar_estado();
        let _ = self.avisos.send(ir_ipc::Aviso::EstadoMudou(self.estado()));
        Resposta::Feito
    }

    /// Retoma: volta a discar na hora, e a aceitar o par.
    pub(super) fn retomar(&mut self) -> Resposta {
        info!("compartilhamento retomado");
        self.pausa = None;
        self.alcance.esquecer_esperas();
        self.connect_if_possible();
        let _ = self.avisos.send(ir_ipc::Aviso::EstadoMudou(self.estado()));
        Resposta::Feito
    }

    /// Se a pausa impede discar agora — daqui ou do outro lado.
    pub(super) const fn pausado(&self) -> bool {
        self.pausa.is_some()
    }

    /// Se um enlace que acabou de subir deve ser recusado por causa da pausa daqui.
    ///
    /// A pausa do outro lado, ao contrário, acaba quando ele liga: foi retomado lá.
    pub(super) fn recusar_pela_pausa(&mut self) -> bool {
        match self.pausa {
            Some(Pausa::Aqui) => true,
            Some(Pausa::NoPar) => {
                info!("o outro computador retomou o compartilhamento");
                self.pausa = None;
                false
            }
            None => false,
        }
    }

    /// A sessão com o par subiu: se ele estava marcado como pausado, não está mais.
    ///
    /// Sem isto, a marca só saía quando um enlace **novo** subia — e uma sessão refeita sobre o
    /// enlace que já existia (a troca de papel) deixava "pausou o compartilhamento" na tela com a
    /// conexão pronta.
    pub(crate) fn par_retomou(&mut self) {
        if self.pausa == Some(Pausa::NoPar) {
            info!("o outro computador voltou a compartilhar");
            self.pausa = None;
        }
    }

    /// A sessão caiu: guarda o motivo para a tela, e entende a pausa do outro lado.
    pub(crate) fn on_queda(&mut self, motivo: LinkDown) {
        self.voltas.esquecer();
        self.ultima_queda = Some(ir_painel::motivo_da_queda(motivo));
        if motivo == LinkDown::PeerClosed(ir_proto::message::DisconnectReason::UserRequested)
            && self.pausa.is_none()
        {
            info!("o outro computador pausou o compartilhamento");
            self.pausa = Some(Pausa::NoPar);
        }
    }
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use ir_proto::carrier::Carrier;
    use ir_proto::message::DisconnectReason;
    use ir_session::{Input, Role};

    use super::*;
    use crate::actor::bancada::{Bancada, Feito};

    #[test]
    fn pausar_derruba_avisa_e_para_de_discar() {
        let mut bancada = Bancada::nova(Role::Client);
        bancada.daemon.alcance.subiu(Carrier::Udp);
        bancada.daemon.drive(Input::CarrierUp(Carrier::Udp));
        let _ = bancada.rede.feitos();

        assert_eq!(bancada.daemon.pausar(), Resposta::Feito);

        assert_eq!(bancada.daemon.session.phase(), Phase::Offline);
        let feitos = bancada.rede.feitos();
        assert!(feitos.contains(&Feito::Desconectou), "{feitos:?}");
        let estado = bancada.daemon.estado();
        assert_eq!(estado.pausa, Some(Pausa::Aqui));
        assert_eq!(estado.ultima_queda, Some(MotivoDaQueda::PedidoPeloUsuario));
        assert!(bancada.daemon.pausado(), "a reconexão não disca");
        assert!(bancada.daemon.recusar_pela_pausa(), "quem ligar é recusado");
    }

    #[test]
    fn retomar_volta_a_aceitar() {
        let mut bancada = Bancada::nova(Role::Client);
        let _ = bancada.daemon.pausar();
        let _ = bancada.daemon.retomar();
        assert_eq!(bancada.daemon.estado().pausa, None);
        assert!(!bancada.daemon.recusar_pela_pausa());
    }

    #[test]
    fn a_pausa_do_par_e_entendida_e_acaba_quando_ele_liga() {
        let mut bancada = Bancada::nova(Role::Server);
        bancada
            .daemon
            .on_queda(LinkDown::PeerClosed(DisconnectReason::UserRequested));
        let estado = bancada.daemon.estado();
        assert_eq!(estado.pausa, Some(Pausa::NoPar));
        assert_eq!(estado.ultima_queda, Some(MotivoDaQueda::ParPausou));

        assert!(!bancada.daemon.recusar_pela_pausa(), "ele retomou e ligou");
        assert_eq!(bancada.daemon.estado().pausa, None);
    }

    #[test]
    fn a_sessao_refeita_no_mesmo_enlace_tira_a_pausa_do_par() {
        // A troca de papel refaz a sessão sem enlace novo: "pausou" ficava na tela, com a conexão
        // pronta.
        let mut bancada = Bancada::nova(Role::Server);
        bancada
            .daemon
            .on_queda(LinkDown::PeerClosed(DisconnectReason::UserRequested));
        bancada
            .daemon
            .out
            .push(ir_session::Command::Notify(ir_session::Notice::Connected {
                peer: ir_proto::peer::MachineName::new("fedora").expect("nome"),
                carrier: Carrier::Rfcomm,
            }));
        bancada.daemon.apply_commands();
        assert_eq!(bancada.daemon.estado().pausa, None);
    }

    #[test]
    fn trocar_de_papel_nao_parece_pausa_ao_par() {
        let mut bancada = Bancada::nova(Role::Server);
        bancada
            .daemon
            .on_queda(LinkDown::PeerClosed(DisconnectReason::Reconfiguring));
        assert_eq!(bancada.daemon.estado().pausa, None);
    }

    #[test]
    fn uma_queda_comum_nao_e_pausa() {
        let mut bancada = Bancada::nova(Role::Server);
        bancada.daemon.on_queda(LinkDown::Timeout);
        assert_eq!(bancada.daemon.estado().pausa, None);
        assert_eq!(
            bancada.daemon.estado().ultima_queda,
            Some(MotivoDaQueda::ParNaoRespondeu)
        );
    }

    #[test]
    fn o_portador_fixado_fica_gravado_e_volta_na_subida() {
        let mut bancada = Bancada::nova(Role::Server);
        assert_eq!(
            bancada
                .daemon
                .fixar_portador(Some(ir_ipc::Portador::Bluetooth)),
            Resposta::Feito
        );
        assert_eq!(
            bancada.daemon.config.portador_fixado.as_deref(),
            Some("bluetooth")
        );
        let relida = crate::config::load_config(&bancada.dir).expect("relê");
        assert_eq!(
            ir_painel::portador_do_texto(relida.portador_fixado.as_deref()),
            Some(ir_ipc::Portador::Bluetooth),
            "depois de reiniciar, a preferência continua"
        );
        assert_eq!(bancada.daemon.fixar_portador(None), Resposta::Feito);
        assert_eq!(bancada.daemon.config.portador_fixado, None);
    }
}
