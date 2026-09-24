//! As traduções entre o vocabulário da sessão e o da interface — e o do agente.
//!
//! Uma por conceito, num lugar só: a borda, a política e o portador tinham nome em três vocabulários
//! (protocolo, interface, arquivo de configuração), e cada módulo do serviço convertia do seu jeito.

use ir_ipc::{Borda, ComandoDoAgente, LinkState, Politica, Portador};
use ir_proto::carrier::Carrier;
use ir_proto::screens::Edge;
use ir_session::{Injection, Phase, Policy};

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

/// A borda do protocolo, no vocabulário da interface.
#[must_use]
pub const fn borda_de(edge: Edge) -> Borda {
    match edge {
        Edge::Left => Borda::Esquerda,
        Edge::Right => Borda::Direita,
        Edge::Top => Borda::Acima,
        Edge::Bottom => Borda::Abaixo,
    }
}

/// O portador do protocolo, no vocabulário da interface.
#[must_use]
pub const fn portador_de(carrier: Carrier) -> Portador {
    match carrier {
        Carrier::Rfcomm => Portador::Bluetooth,
        Carrier::Udp => Portador::RedeLocal,
        Carrier::Tcp => Portador::RedeDeArquivos,
    }
}

/// O portador como fica no arquivo de configuração.
#[must_use]
pub const fn texto_do_portador(portador: Portador) -> &'static str {
    match portador {
        Portador::Bluetooth => "bluetooth",
        Portador::RedeLocal | Portador::RedeDeArquivos => "rede",
    }
}

/// O portador fixado no arquivo de configuração, se o texto for um dos conhecidos.
#[must_use]
pub fn portador_do_texto(texto: Option<&str>) -> Option<Portador> {
    match texto? {
        "bluetooth" => Some(Portador::Bluetooth),
        "rede" => Some(Portador::RedeLocal),
        _ => None,
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

/// Um comando de injeção da sessão, no vocabulário do agente.
#[must_use]
pub const fn comando_do_agente(injection: Injection) -> Option<ComandoDoAgente> {
    Some(match injection {
        Injection::Key { usage, pressed } => ComandoDoAgente::Tecla {
            usage,
            pressionada: pressed,
        },
        Injection::Button { button, pressed } => ComandoDoAgente::Botao {
            botao: button,
            pressionado: pressed,
        },
        Injection::Wheel(delta) => ComandoDoAgente::Roda(delta),
        Injection::Pointer(position) => ComandoDoAgente::Ponteiro(position),
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn o_portador_vai_ao_arquivo_e_volta_igual() {
        for portador in [Portador::Bluetooth, Portador::RedeLocal] {
            assert_eq!(
                portador_do_texto(Some(texto_do_portador(portador))),
                Some(portador)
            );
        }
        assert_eq!(portador_do_texto(Some("pombo-correio")), None);
        assert_eq!(portador_do_texto(None), None);
    }

    #[test]
    fn a_politica_vai_e_volta() {
        for policy in [Policy::Both, Policy::OnlyControls, Policy::OnlyControlled] {
            assert_eq!(policy_de(politica_de(policy)), policy);
        }
    }
}
