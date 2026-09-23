//! O que a interface precisa saber.

use ir_proto::carrier::Carrier;
use ir_proto::ids::RadioAddress;
use ir_proto::message::ErrorCode;
use ir_proto::peer::MachineName;
use ir_proto::screens::Edge;

use super::LinkDown;
use crate::session::Route;

/// O que a interface precisa saber.
///
/// Existe para que o estado seja **observável** — o que faltou no v1 e tornou qualquer
/// diagnóstico impossível (`docs/00-licoes-do-v1.md` §6).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Notice {
    /// A sessão foi estabelecida.
    Connected {
        /// Nome legível do par.
        peer: MachineName,
        /// Por qual portador.
        carrier: Carrier,
    },
    /// A sessão terminou.
    Disconnected {
        /// Por quê.
        reason: LinkDown,
        /// Se a sessão vai tentar voltar sozinha.
        will_retry: bool,
    },
    /// O controle mudou de lado.
    ControlMoved {
        /// `true` quando o controle está agora no par.
        remote: bool,
    },
    /// O portador ativo mudou.
    CarrierChanged {
        /// O novo.
        carrier: Carrier,
        /// Por que ele foi escolhido.
        why: CarrierChoice,
    },
    /// O cliente reconciliou um estado divergente.
    ///
    /// Muitos destes seguidos indicam perda no canal confiável, e o número aparece no
    /// diagnóstico.
    Reconciled {
        /// Quantas teclas foram soltas.
        released: u8,
        /// Quantas foram pressionadas.
        pressed: u8,
    },
    /// Uma medida de ida e volta até o par.
    ///
    /// A sessão produz amostras; quem calcula mediana e p99 é a periferia, que é quem tem
    /// histórico. Sem isto, "latência observável" de `docs/01-visao-e-escopo.md` §5 não
    /// existiria.
    LatencySample(crate::time::Millis),
    /// Erro de protocolo.
    ProtocolError {
        /// O código.
        code: ErrorCode,
        /// Se a sessão terminou por causa dele.
        fatal: bool,
    },
    /// A borda que dá para o par mudou, e é esta que vale agora.
    ///
    /// No servidor, porque o usuário escolheu; no cliente, porque o servidor anunciou. O serviço
    /// grava: sem isto, o cliente voltaria à borda velha na próxima subida, até reconectar.
    EdgeChanged {
        /// A borda desta tela que dá para a tela do par.
        edge: Edge,
    },
    /// A rota da sessão de pé mudou: um portador entrou ou saiu, sem refazer a sessão.
    ///
    /// Distinto de [`Self::CarrierChanged`], que anuncia um aperto de mão novo por outro portador.
    /// Aqui nada foi solto e nada recomeçou — é o que a rota dupla existe para permitir.
    RouteChanged {
        /// A rota que vale agora.
        route: Route,
        /// Por que ela é esta.
        why: CarrierChoice,
    },
    /// O par contou o endereço do rádio dele.
    ///
    /// É o que permite discar o Bluetooth para um par que só era conhecido pela rede. Quem disca é
    /// a periferia: a sessão só repassa o que ouviu.
    PeerRadio(RadioAddress),
    /// O par contou como está a economia de energia do Wi-Fi dele.
    PeerNetworkPower(ir_proto::message::NetworkPowerSaving),
    /// O par pediu que a economia de energia do Wi-Fi daqui seja desligada. Quem aplica é a
    /// periferia; a sessão só repassa.
    NetworkPowerFixRequested,
    /// O pedido de desligar a economia no par não pôde sair: sem sessão, ou o par não entende.
    PeerCannotFixNetworkPower,
}

/// Por que um portador foi escolhido.
///
/// A política é **uma só**, sempre a mesma (`docs/01-visao-e-escopo.md` §5). Este tipo é o
/// que torna a decisão visível em vez de silenciosa — foi a degradação escondida que tornou
/// o v1 impossível de diagnosticar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum CarrierChoice {
    /// Bluetooth estava disponível e é o preferido.
    Preferred,
    /// Bluetooth não estava disponível; caiu para a rede.
    FellBackToNetwork,
    /// O usuário fixou este portador.
    PinnedByUser,
    /// Bluetooth e rede juntos: cada quadro vai pelos dois, e vale o que chegar primeiro.
    Redundant,
}

impl CarrierChoice {
    /// Frase para a interface.
    #[must_use]
    pub const fn description(self) -> &'static str {
        match self {
            Self::Preferred => "Bluetooth disponível, que é o preferido para teclado e mouse",
            Self::FellBackToNetwork => "Bluetooth indisponível; usando a rede local",
            Self::PinnedByUser => "fixado nas preferências",
            Self::Redundant => {
                "Bluetooth e rede local juntos: cada comando vai pelos dois e vale o que chegar primeiro"
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_carrier_choice_can_be_explained_to_the_user() {
        for choice in [
            CarrierChoice::Preferred,
            CarrierChoice::FellBackToNetwork,
            CarrierChoice::PinnedByUser,
            CarrierChoice::Redundant,
        ] {
            assert!(
                !choice.description().is_empty(),
                "{choice:?} sem explicação"
            );
        }
    }
}
