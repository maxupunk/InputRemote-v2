//! O que o sistema operacional avisa: suspender, retomar e trocar de sessão.
//!
//! Suspender é uma queda anunciada: o par é avisado com o motivo certo ("o outro computador foi
//! suspenso", que ele tenta de novo sozinho) e tudo é solto **antes** de a máquina dormir —
//! depois, nem o serviço nem o agente estão acordados para soltar nada. Retomar é o contrário:
//! discar na hora, sem esperar a rodada de reconexão, e reler a placa de rede, que o driver pode
//! ter reconfigurado. Trocar de sessão (login, logoff, troca rápida de usuário) é quando o agente
//! morre e precisa nascer na sessão nova.
//!
//! No Windows os avisos vêm do SCM (`service.rs`); no Linux, de um gancho do `systemd` em
//! `system-sleep`, que manda `SIGUSR1` antes de dormir e `SIGUSR2` ao acordar.

use ir_session::LinkDown;
use tracing::info;

use super::Daemon;

pub(crate) use ir_servico::EventoDoSistema;

impl Daemon {
    /// Um aviso do sistema, na vez do ator.
    pub(super) fn on_sistema(&mut self, evento: EventoDoSistema) {
        match evento {
            EventoDoSistema::Suspendendo => self.suspender(),
            EventoDoSistema::Retomou => self.retomar_do_sono(),
            EventoDoSistema::SessaoMudou => self.garantir_agente_agora(),
            EventoDoSistema::TelaBloqueada => self.bloquear_o_par_junto(),
        }
    }

    /// A tela daqui bloqueou: o par bloqueia junto, se a pessoa quis assim. Quem decide se aceita
    /// é o par, pela política dele — um computador que nunca é controlado não bloqueia a pedido.
    pub(super) fn bloquear_o_par_junto(&mut self) {
        if !self.config.bloquear_juntos {
            return;
        }
        info!("a tela daqui bloqueou: pedindo ao par que bloqueie a dele");
        self.drive(ir_session::Input::LockPeerScreen);
    }

    /// O par bloqueou a tela dele, e pediu que esta bloqueie também.
    pub(crate) fn bloquear_a_tela(&mut self) {
        info!("o par bloqueou a tela dele: bloqueando esta");
        if let Some(agente) = self.comandos_do_agente() {
            let _ = agente.send(ir_ipc::ComandoDoAgente::BloquearTela);
        } else {
            // O serviço é root: bloqueia todas as sessões gráficas desta máquina.
            #[cfg(target_os = "linux")]
            ir_servico::logind::bloquear_sessoes();
        }
    }

    /// Solta tudo e avisa o par antes de dormir.
    fn suspender(&mut self) {
        info!("a máquina vai suspender: soltando tudo e avisando o par");
        self.dormindo = true;
        self.encerrar_sessao(LinkDown::Suspending);
        self.notar_estado();
    }

    /// Acordou: disca já, e confere a placa de rede.
    fn retomar_do_sono(&mut self) {
        info!("a máquina acordou: reconectando");
        self.dormindo = false;
        self.alcance.esquecer_esperas();
        self.reconnect_if_needed();
        self.verificar_economia();
        self.garantir_agente_agora();
    }
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use ir_proto::carrier::Carrier;
    use ir_session::{Input, Phase};

    use super::*;
    use crate::actor::bancada::Bancada;

    #[test]
    fn suspender_derruba_a_sessao_e_para_de_discar_ate_acordar() {
        let mut bancada = Bancada::nova();
        bancada.daemon.drive(Input::CarrierUp(Carrier::Udp));
        assert_ne!(bancada.daemon.session.phase(), Phase::Offline);

        bancada.daemon.on_sistema(EventoDoSistema::Suspendendo);

        assert_eq!(bancada.daemon.session.phase(), Phase::Offline);
        assert!(bancada.daemon.dormindo);
        bancada.daemon.on_sistema(EventoDoSistema::Retomou);
        assert!(!bancada.daemon.dormindo);
    }

    #[test]
    fn travar_a_borda_vale_e_aparece_na_janela() {
        let mut bancada = Bancada::nova();
        let resposta = bancada.daemon.tratar(
            ir_ipc::Pedido::TravarBorda(true),
            ir_transferencia::Leitor::Proprio,
        );
        assert_eq!(resposta, ir_ipc::Resposta::Feito);
        assert!(bancada.daemon.estado().borda_travada);
    }

    #[test]
    fn bloquear_juntos_nasce_ligado_e_desligar_fica_gravado() {
        let mut bancada = Bancada::nova();
        assert!(bancada.daemon.estado().bloquear_juntos);
        let _ = bancada.daemon.tratar(
            ir_ipc::Pedido::BloquearJuntos(false),
            ir_transferencia::Leitor::Proprio,
        );
        assert!(!bancada.daemon.estado().bloquear_juntos);
        let relida = crate::config::load_config(&bancada.dir).expect("relê");
        assert!(!relida.bloquear_juntos);
    }

    #[test]
    fn so_quem_tem_o_teclado_pede_ao_par_que_bloqueie() {
        let mut bancada = Bancada::nova();
        bancada.daemon.drive(Input::CarrierUp(Carrier::Udp));
        let _ = bancada.rede.feitos();
        bancada.daemon.on_sistema(EventoDoSistema::TelaBloqueada);
        assert!(
            bancada.rede.feitos().is_empty(),
            "o controlado não tranca quem digita"
        );
    }
}
