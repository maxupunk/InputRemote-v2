//! Canal 0 — controle.
//!
//! Handshake, arranjo de telas, travessia, heartbeat e encerramento. Bidirecional e
//! confiável. Ver `docs/03-protocolo.md` §6.

use serde::{Deserialize, Serialize};

use crate::ids::{MachineId, RadioAddress};
use crate::input::{InputState, PointerPosition};
use crate::peer::{Capabilities, MachineName};
use crate::screens::{Edge, ScreenLayout};
use crate::version::ProtocolVersion;

/// Mensagem do canal de controle.
///
/// A ordem das variantes **é o formato de fio**: `postcard` codifica a posição, não o nome.
/// Acrescentar variante no fim é seguro; inserir no meio ou reordenar quebra
/// compatibilidade em silêncio e exige incremento de `version::CURRENT`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum Control {
    /// Primeira mensagem de quem inicia.
    Hello(Greeting),
    /// Resposta de quem aceita, com a versão acordada.
    HelloAck(Greeting),
    /// O arranjo de telas de quem envia, sempre que ele muda.
    Screens(ScreenLayout),
    /// De que lado fica o par, do ponto de vista de quem envia.
    ///
    /// Só o servidor envia — ao estabelecer a sessão e a cada troca —, porque a borda é dele: é
    /// ele quem tem o teclado e o mouse. O cliente usa a borda oposta, e um anúncio que chegue ao
    /// servidor é ignorado.
    EdgeConfig {
        /// A borda desta tela que dá para a tela do par.
        peer_edge: Edge,
    },
    /// O controle passou para o par.
    ///
    /// Carrega o estado de entrada completo justamente para que o cliente comece
    /// sincronizado, sem depender de um snapshot posterior.
    EnterScreen {
        /// Por qual borda do cliente o ponteiro entra.
        entering_edge: Edge,
        /// Onde o ponteiro aparece.
        position: PointerPosition,
        /// O que está pressionado no momento da travessia.
        state: InputState,
    },
    /// O controle voltou para quem envia.
    LeaveScreen {
        /// Por qual borda do cliente o ponteiro saiu.
        leaving_edge: Edge,
        /// Última posição conhecida.
        position: PointerPosition,
    },
    /// Estado completo de entrada, para reconciliação idempotente.
    ///
    /// A rede de segurança contra tecla presa (`docs/03-protocolo.md` §7).
    StateSnapshot {
        /// O que deveria estar pressionado.
        state: InputState,
        /// Onde o ponteiro deveria estar.
        position: PointerPosition,
    },
    /// Sonda de latência e de vida.
    Ping {
        /// Carimbo monotônico de quem envia, em microssegundos.
        ///
        /// Só quem enviou interpreta este número; para o outro lado ele é opaco e é
        /// devolvido intacto. Assim não é preciso relógio comum entre as máquinas.
        stamp_micros: u64,
    },
    /// Resposta à sonda, devolvendo o carimbo intacto.
    Pong {
        /// O mesmo valor recebido no [`Control::Ping`].
        stamp_micros: u64,
    },
    /// Confirmação pura, sem outro conteúdo.
    ///
    /// Enviada a cada 20 ms enquanto houver mensagem pendente e não houver tráfego de volta
    /// para carregar a confirmação. O conteúdo vai no campo `ack` do quadro.
    AckOnly,
    /// Encerramento anunciado, com motivo.
    Bye {
        /// Por que a sessão está terminando.
        reason: DisconnectReason,
    },
    /// Erro de protocolo, por código.
    Error {
        /// O que deu errado, em código fechado.
        code: ErrorCode,
        /// Se a sessão termina por causa disto.
        fatal: bool,
    },
    /// Por onde mais quem envia pode ser alcançado.
    ///
    /// Mandado ao estabelecer a sessão, e de novo quando muda. É o que deixa a rota dupla
    /// (`docs/03-protocolo.md` §2.1) nascer de um pareamento feito pela rede: sem isto, quem só
    /// conhece o par pela rede não teria para onde discar o Bluetooth. O endereço de rede não
    /// viaja aqui — ele é achado pela descoberta, a partir do [`MachineId`] do par.
    Reach {
        /// O endereço do rádio Bluetooth de quem envia.
        radio: RadioAddress,
    },
    /// Como está a economia de energia do Wi-Fi de quem envia. Desde a versão 3.
    ///
    /// Com a economia ligada a placa cochila entre pacotes, e os comandos pela rede chegam em
    /// rajadas: o ponteiro flui, trava e volta a fluir. Quem está olhando a tela quase sempre é o
    /// outro computador, e é lá que o aviso precisa aparecer.
    NetworkPower(NetworkPowerSaving),
    /// Desligue a economia de energia do seu Wi-Fi. Desde a versão 3.
    ///
    /// Só isto: um par autenticado pode pedir que a placa daqui não cochile, e nada mais sobre a
    /// configuração da máquina. O botão que manda o pedido está na tela do outro computador.
    DisableNetworkPowerSaving,
}

/// A economia de energia do Wi-Fi de uma máquina.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum NetworkPowerSaving {
    /// Desligada, ou não há Wi-Fi.
    Off,
    /// Ligada agora.
    On,
    /// Desligada na tomada e ligada na bateria.
    OnBattery,
}

/// Identificação trocada no handshake.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Greeting {
    /// Versão que quem envia fala.
    pub version: ProtocolVersion,
    /// Identificador da instalação.
    pub machine: MachineId,
    /// Nome legível, para a interface.
    pub name: MachineName,
    /// O que quem envia declara ser capaz de fazer.
    pub capabilities: Capabilities,
}

/// Por que uma sessão terminou.
///
/// Existe para a interface poder dizer "o outro computador foi suspenso" em vez de
/// "conexão perdida". No v1, toda queda parecia igual, e diagnosticar era impossível.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum DisconnectReason {
    /// O usuário pediu.
    UserRequested,
    /// O serviço está parando.
    ServiceStopping,
    /// A máquina vai suspender.
    Suspending,
    /// Reconfiguração exige reconectar.
    Reconfiguring,
    /// Erro de protocolo — o detalhe fica no log local, não vai para o par.
    ProtocolError,
    /// O par parou de responder.
    Timeout,
    /// Troca de portador em andamento; a sessão volta em seguida.
    SwitchingCarrier,
}

impl DisconnectReason {
    /// Se vale tentar reconectar sozinho depois disto.
    ///
    /// Reconectar após `UserRequested` seria desobedecer o usuário; após `ProtocolError`
    /// seria repetir o mesmo erro em laço.
    #[must_use]
    pub const fn should_retry(self) -> bool {
        matches!(
            self,
            Self::Timeout | Self::Suspending | Self::SwitchingCarrier
        )
    }

    /// Frase para a interface.
    #[must_use]
    pub const fn description(self) -> &'static str {
        match self {
            Self::UserRequested => "encerrada pelo usuário",
            Self::ServiceStopping => "o serviço do outro computador está parando",
            Self::Suspending => "o outro computador foi suspenso",
            Self::Reconfiguring => "reconfiguração em andamento",
            Self::ProtocolError => "erro de protocolo",
            Self::Timeout => "o outro computador parou de responder",
            Self::SwitchingCarrier => "trocando de meio de conexão",
        }
    }
}

/// Erro de protocolo, em código fechado.
///
/// **Nunca carrega texto vindo do decodificador.** O par recebe o código; o detalhe fica no
/// log local (`docs/04-seguranca.md` §7 e `docs/09-padroes-de-codigo.md` §5). Um erro
/// detalhado enviado de volta é um oráculo para quem sonda o serviço.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum ErrorCode {
    /// Os bytes recebidos não formam mensagem válida.
    DecodeFailed,
    /// A mensagem é conhecida mas não é aceita nesta versão.
    UnsupportedMessage,
    /// A mensagem chegou por um canal que não a permite.
    ChannelViolation,
    /// A mensagem passou do tamanho do portador.
    TooLarge,
    /// Os dois lados discordam sobre o estado da sessão.
    StateDesync,
    /// Falha interna de quem envia, sem detalhe.
    Internal,
}

impl ErrorCode {
    /// Se este erro sempre encerra a sessão, independentemente do campo `fatal`.
    ///
    /// Um desacordo de canal ou de decodificação significa que as duas pontas não falam a
    /// mesma língua; prosseguir seria agir sob ambiguidade — e agir sob ambiguidade, neste
    /// produto, é digitar a coisa errada na máquina do outro.
    #[must_use]
    pub const fn is_always_fatal(self) -> bool {
        matches!(
            self,
            Self::DecodeFailed | Self::ChannelViolation | Self::UnsupportedMessage
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_transient_reasons_ask_for_a_retry() {
        use DisconnectReason as R;
        assert!(R::Timeout.should_retry());
        assert!(R::Suspending.should_retry());
        assert!(R::SwitchingCarrier.should_retry());
        assert!(
            !R::UserRequested.should_retry(),
            "não se reconecta contra a vontade"
        );
        assert!(
            !R::ProtocolError.should_retry(),
            "reconectar repetiria o erro em laço"
        );
        assert!(!R::ServiceStopping.should_retry());
        assert!(!R::Reconfiguring.should_retry());
    }

    #[test]
    fn every_reason_has_a_human_description() {
        use DisconnectReason as R;
        for reason in [
            R::UserRequested,
            R::ServiceStopping,
            R::Suspending,
            R::Reconfiguring,
            R::ProtocolError,
            R::Timeout,
            R::SwitchingCarrier,
        ] {
            assert!(!reason.description().is_empty(), "{reason:?} sem descrição");
        }
    }

    #[test]
    fn language_disagreements_are_always_fatal() {
        assert!(ErrorCode::DecodeFailed.is_always_fatal());
        assert!(ErrorCode::ChannelViolation.is_always_fatal());
        assert!(ErrorCode::UnsupportedMessage.is_always_fatal());
        assert!(
            !ErrorCode::StateDesync.is_always_fatal(),
            "desacordo de estado é corrigível"
        );
        assert!(!ErrorCode::Internal.is_always_fatal());
        assert!(!ErrorCode::TooLarge.is_always_fatal());
    }

    #[test]
    fn ping_and_pong_carry_an_opaque_stamp() {
        // O carimbo é do emissor e volta intacto: nenhuma das pontas precisa interpretar o
        // relógio da outra.
        let sent = Control::Ping {
            stamp_micros: 1_234_567,
        };
        let echoed = match sent {
            Control::Ping { stamp_micros } => Control::Pong { stamp_micros },
            _ => unreachable!(),
        };
        assert_eq!(
            echoed,
            Control::Pong {
                stamp_micros: 1_234_567
            }
        );
    }
}
