//! As traduções entre o vocabulário da sessão e o da interface.
//!
//! Uma por conceito, num lugar só: a borda, a política e o portador tinham nome em três vocabulários
//! (protocolo, interface, arquivo de configuração), e cada módulo do serviço convertia do seu jeito.
//! A borda e o portador entre protocolo e interface são `From` do próprio `ir-ipc`; o texto do
//! arquivo é do `ir-configuracao`. Aqui fica o resto.

use ir_ipc::{LinkState, Politica};
use ir_session::{Phase, Policy};

/// A fase da sessão, traduzida para o enlace que a interface mostra.
#[must_use]
pub const fn link_state(phase: Phase) -> LinkState {
    match phase {
        Phase::Offline => LinkState::Desconectado,
        Phase::Handshaking => LinkState::Conectando,
        Phase::Ready => LinkState::Pronto,
        Phase::Sending => LinkState::Controlando,
        Phase::Receiving => LinkState::Controlado,
    }
}

/// A política da sessão, no vocabulário da interface.
#[must_use]
pub const fn politica_de(policy: Policy) -> Politica {
    match policy {
        Policy::Both => Politica::Ambos,
        Policy::OnlyControls => Politica::SoEste,
        Policy::OnlyControlled => Politica::SoOOutro,
    }
}

/// A política da interface, no vocabulário da sessão.
#[must_use]
pub const fn policy_de(politica: Politica) -> Policy {
    match politica {
        Politica::Ambos => Policy::Both,
        Politica::SoEste => Policy::OnlyControls,
        Politica::SoOOutro => Policy::OnlyControlled,
    }
}

/// A economia de energia do Wi-Fi, como a sessão a conta, no vocabulário da tela — só quando
/// atrapalha.
#[must_use]
pub const fn economia_na_tela(
    estado: Option<ir_proto::message::NetworkPowerSaving>,
) -> Option<ir_ipc::EconomiaDoWifi> {
    use ir_proto::message::NetworkPowerSaving as E;
    match estado {
        Some(E::On) => Some(ir_ipc::EconomiaDoWifi::Ligada),
        Some(E::OnBattery) => Some(ir_ipc::EconomiaDoWifi::SoNaBateria),
        Some(E::Off) | None => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_politica_vai_e_volta() {
        for policy in [Policy::Both, Policy::OnlyControls, Policy::OnlyControlled] {
            assert_eq!(policy_de(politica_de(policy)), policy);
        }
    }
}
