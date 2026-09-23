//! O que pode acontecer com a sessão.

use ir_proto::carrier::Carrier;
use ir_proto::frame::Frame;
use ir_proto::input::{Button, HidUsage, PointerDelta, WheelDelta};
use ir_proto::message::DisconnectReason;
use ir_proto::screens::{Edge, ScreenLayout};

/// Tudo que pode acontecer com a sessão.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Input {
    /// Passou tempo. Nada mais aconteceu.
    ///
    /// É como prazos vencem. A periferia entrega um `Tick` quando um temporizador que a
    /// sessão pediu dispara, e também periodicamente — a sessão tolera os dois.
    Tick,

    /// Um portador ficou disponível.
    CarrierUp(Carrier),

    /// Um portador caiu.
    CarrierDown {
        /// Qual.
        carrier: Carrier,
        /// Por quê, do ponto de vista local.
        reason: LinkDown,
    },

    /// Chegou um quadro do par.
    Received {
        /// Por onde chegou.
        carrier: Carrier,
        /// O quadro já decifrado e decodificado.
        frame: Frame,
    },

    /// O ponteiro local se moveu. Só faz sentido no servidor.
    LocalPointer(PointerDelta),

    /// A roda local girou.
    LocalWheel(WheelDelta),

    /// Uma tecla local mudou de estado.
    LocalKey {
        /// A tecla física.
        usage: HidUsage,
        /// `true` para pressionada.
        pressed: bool,
    },

    /// Um botão local mudou de estado.
    LocalButton {
        /// O botão.
        button: Button,
        /// `true` para pressionado.
        pressed: bool,
    },

    /// O usuário acionou o atalho de emergência.
    ///
    /// Devolve o controle e solta tudo imediatamente, mesmo com o enlace saudável. É a saída
    /// de que o usuário precisa quando alguma coisa deu errado e ele não sabe o quê.
    EmergencyRelease,

    /// O arranjo de telas desta máquina mudou.
    LocalScreens(ScreenLayout),

    /// O usuário escolheu por qual borda desta tela se chega ao par.
    ///
    /// Só vale no servidor, que tem o teclado e o mouse e é a fonte de verdade da borda. O cliente
    /// não escolhe: ele usa a oposta da que o servidor anunciar.
    SetPeerEdge(Edge),

    /// O agente desta máquina está pronto para injetar.
    AgentReady,

    /// O agente desta máquina sumiu.
    ///
    /// No Windows acontece a cada logoff e a cada troca rápida de usuário. Não é falha da
    /// sessão; é rotina, e o estado é do serviço justamente por isso
    /// (`docs/02-arquitetura.md` §1.1).
    AgentLost,

    /// O usuário copiou este texto e levou o controle ao par: ofereça-o.
    ///
    /// Vai pelo canal 4, em qualquer portador. Sem sessão estabelecida, é descartado — oferecer
    /// depois, fora do momento da travessia, poria no clipboard do par algo que ele não pediu.
    ClipboardText(super::ClipText),

    /// O endereço do rádio Bluetooth desta máquina ficou conhecido.
    ///
    /// A sessão o conta ao par em [`Control::Reach`](ir_proto::message::Control::Reach) — ao
    /// estabelecer e, se já estiver de pé, na hora. É o que deixa o par discar o Bluetooth quando
    /// os dois se conheceram pela rede, e a rota dupla nascer de qualquer pareamento.
    LocalRadio(ir_proto::ids::RadioAddress),

    /// A periferia viu como está a economia de energia do Wi-Fi desta máquina.
    ///
    /// A sessão conta ao par, que é quem mostra o aviso — quem sente as travadas é quem olha a
    /// tela do outro lado.
    LocalNetworkPower(ir_proto::message::NetworkPowerSaving),

    /// O usuário pediu, daqui, que o par desligue a economia de energia do Wi-Fi dele.
    DisablePeerNetworkPowerSaving,
    /// Peça Ctrl+Alt+Del ao par — pelo botão da janela, além do atalho Ctrl+Alt+End.
    SecureAttention,
    /// A periferia daqui passou a recusar (`true`), ou voltou a aceitar, digitação do par no
    /// desktop protegido — a tela de bloqueio e o UAC.
    LocalProtectedDesktop(bool),
    /// Trave, ou destrave, a borda: com ela travada, o ponteiro não atravessa para o par — só o
    /// atalho Ctrl+Alt+Shift+Espaço leva o controle.
    LockEdge(bool),
    /// A tela daqui bloqueou: peça ao par que bloqueie a dele.
    LockPeerScreen,
}

/// Por que um portador caiu, do ponto de vista local.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum LinkDown {
    /// O par anunciou o encerramento.
    PeerClosed(DisconnectReason),
    /// O par parou de responder.
    Timeout,
    /// O transporte falhou: socket fechado, rádio desligado, cabo removido.
    TransportFailed,
    /// Esta máquina vai suspender.
    Suspending,
    /// O usuário mandou parar.
    UserStopped,
    /// O par começou uma sessão nova sem que a anterior tivesse terminado deste lado.
    ///
    /// É o caso do adeus que se perdeu no caminho. Não se avisa o par de nada: ele já está em
    /// outra sessão, e um adeus desta seria descartado por ser de uma encarnação que acabou.
    PeerRestarted,
    /// O serviço desta máquina está parando: desligamento, atualização.
    ///
    /// Distinto de [`Self::UserStopped`]: o par entende "pedido pelo usuário" como pausa e para
    /// de discar; um serviço que para para atualizar volta em segundos, e o par deve esperá-lo.
    ServiceStopping,
    /// Esta máquina trocou de papel e recomeça a sessão sobre o mesmo enlace.
    ///
    /// Distinto de [`Self::UserStopped`] pelo mesmo motivo de [`Self::ServiceStopping`]: o par que
    /// ouvisse "pedido pelo usuário" mostraria que foi pausado, e a sessão nova chega em seguida.
    Reconfiguring,
}

impl LinkDown {
    /// Se vale tentar reconectar sozinho depois disto.
    #[must_use]
    pub const fn should_retry(self) -> bool {
        match self {
            Self::PeerClosed(reason) => reason.should_retry(),
            Self::Timeout
            | Self::TransportFailed
            | Self::Suspending
            | Self::PeerRestarted
            | Self::ServiceStopping
            | Self::Reconfiguring => true,
            Self::UserStopped => false,
        }
    }

    /// O motivo a anunciar ao par, quando somos nós que encerramos.
    #[must_use]
    pub const fn as_disconnect_reason(self) -> DisconnectReason {
        match self {
            Self::PeerClosed(reason) => reason,
            Self::Timeout => DisconnectReason::Timeout,
            Self::TransportFailed => DisconnectReason::ProtocolError,
            Self::Suspending => DisconnectReason::Suspending,
            Self::UserStopped => DisconnectReason::UserRequested,
            Self::ServiceStopping => DisconnectReason::ServiceStopping,
            // `PeerRestarted` nunca vai ao par; o mais próximo do que aconteceu é uma reconfiguração.
            Self::Reconfiguring | Self::PeerRestarted => DisconnectReason::Reconfiguring,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_peer_that_restarted_is_worth_rejoining() {
        // O par já está numa sessão nova esperando por nós: desistir seria deixá-lo sozinho.
        assert!(LinkDown::PeerRestarted.should_retry());
    }

    #[test]
    fn only_a_user_stop_refuses_to_retry() {
        assert!(!LinkDown::UserStopped.should_retry());
        assert!(LinkDown::Timeout.should_retry());
        assert!(LinkDown::TransportFailed.should_retry());
        assert!(LinkDown::Suspending.should_retry());
    }

    #[test]
    fn a_peer_close_inherits_the_peers_retry_policy() {
        assert!(LinkDown::PeerClosed(DisconnectReason::Timeout).should_retry());
        assert!(!LinkDown::PeerClosed(DisconnectReason::UserRequested).should_retry());
        assert!(!LinkDown::PeerClosed(DisconnectReason::ProtocolError).should_retry());
    }

    #[test]
    fn every_local_reason_maps_to_something_the_peer_understands() {
        use DisconnectReason as D;
        assert_eq!(LinkDown::Timeout.as_disconnect_reason(), D::Timeout);
        assert_eq!(LinkDown::Suspending.as_disconnect_reason(), D::Suspending);
        assert_eq!(
            LinkDown::UserStopped.as_disconnect_reason(),
            D::UserRequested
        );
        assert_eq!(
            LinkDown::PeerClosed(D::Reconfiguring).as_disconnect_reason(),
            D::Reconfiguring,
            "o motivo do par é repassado intacto"
        );
    }
}
