//! Os pedaços de estado que a sessão guarda, cada um com sua responsabilidade.

use ir_proto::carrier::Carrier;
use ir_proto::ids::MachineId;
use ir_proto::peer::{Capabilities, MachineName};
use ir_proto::version::ProtocolVersion;

use crate::event::CarrierChoice;
use crate::time::Timestamp;

/// Quem esta máquina é, do ponto de vista do protocolo.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalIdentity {
    /// Identificador da instalação.
    pub machine: MachineId,
    /// Nome legível, para a interface do par.
    pub name: MachineName,
    /// O que esta máquina declara ser capaz de fazer.
    pub capabilities: Capabilities,
}

/// Quem o par é, depois do handshake.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PeerInfo {
    /// Identificador da instalação do par.
    pub machine: MachineId,
    /// Nome legível do par.
    pub name: MachineName,
    /// O que ele declarou.
    pub capabilities: Capabilities,
    /// A versão acordada.
    pub version: ProtocolVersion,
}

/// Quais portadores estão disponíveis agora.
///
/// Um campo por portador, e não um vetor: são três, o conjunto nunca cresce, e um campo
/// nomeado não tem caminho de indexação inválida.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CarrierSet {
    rfcomm: bool,
    udp: bool,
    tcp: bool,
}

impl CarrierSet {
    /// Nenhum portador disponível.
    pub const NONE: Self = Self {
        rfcomm: false,
        udp: false,
        tcp: false,
    };

    /// Marca um portador como disponível ou não.
    pub const fn set(&mut self, carrier: Carrier, available: bool) {
        match carrier {
            Carrier::Rfcomm => self.rfcomm = available,
            Carrier::Udp => self.udp = available,
            Carrier::Tcp => self.tcp = available,
        }
    }

    /// Se este portador está disponível.
    #[must_use]
    pub const fn has(self, carrier: Carrier) -> bool {
        match carrier {
            Carrier::Rfcomm => self.rfcomm,
            Carrier::Udp => self.udp,
            Carrier::Tcp => self.tcp,
        }
    }

    /// Se nenhum portador de entrada está disponível.
    #[must_use]
    pub const fn has_no_input_carrier(self) -> bool {
        !self.rfcomm && !self.udp
    }

    /// O portador de entrada a usar, e por quê.
    ///
    /// **A política única** de `docs/01-visao-e-escopo.md` §5, num lugar só: Bluetooth se
    /// existir, senão UDP, senão nada. O v1 tinha três políticas de degradação diferentes,
    /// uma por modo, e por isso ninguém conseguia prever nem reproduzir o comportamento
    /// (`docs/00-licoes-do-v1.md` §6).
    ///
    /// Devolve também o motivo, para que a escolha seja **visível** na interface em vez de
    /// silenciosa.
    #[must_use]
    pub const fn pick_input_carrier(
        self,
        pinned: Option<Carrier>,
    ) -> Option<(Carrier, CarrierChoice)> {
        if let Some(carrier) = pinned {
            // Fixar desliga a degradação: falhar vira falhar, não vira "outro caminho".
            return if self.has(carrier) && carrier.carries_input() {
                Some((carrier, CarrierChoice::PinnedByUser))
            } else {
                None
            };
        }
        if self.rfcomm {
            return Some((Carrier::Rfcomm, CarrierChoice::Preferred));
        }
        if self.udp {
            return Some((Carrier::Udp, CarrierChoice::FellBackToNetwork));
        }
        None
    }
}

/// Os carimbos que a sessão consulta para saber se algum prazo venceu.
///
/// Nenhum deles é lido do relógio: todos vêm do instante que a periferia passou para
/// [`Session::step`](crate::Session::step).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Clock {
    /// Quando chegou a última mensagem do par.
    pub last_rx: Timestamp,
    /// Quando saiu o último `Ping`.
    pub last_heartbeat: Timestamp,
    /// Quando saiu o último `StateSnapshot`.
    pub last_snapshot: Timestamp,
    /// Quando saiu a última amostra de ponteiro.
    pub last_pointer: Timestamp,
    /// Quando saiu a última confirmação pura.
    pub last_bare_ack: Timestamp,
    /// Quando chegou o último quadro pelo Bluetooth.
    ///
    /// Os dois carimbos por portador não decidem prazo nenhum — quem decide é [`Self::last_rx`],
    /// que conta a rota inteira. Eles existem para a interface poder dizer qual dos dois
    /// portadores da rota dupla parou de responder.
    pub last_rx_rfcomm: Timestamp,
    /// Quando chegou o último quadro pela rede.
    pub last_rx_udp: Timestamp,
}

impl Clock {
    /// Todos os carimbos no instante dado.
    ///
    /// Chamado ao estabelecer a sessão: começar com zero faria todos os prazos vencerem de
    /// uma vez no primeiro `Tick`.
    #[must_use]
    pub const fn started_at(now: Timestamp) -> Self {
        Self {
            last_rx: now,
            last_heartbeat: now,
            last_snapshot: now,
            last_pointer: now,
            last_bare_ack: now,
            last_rx_rfcomm: now,
            last_rx_udp: now,
        }
    }

    /// Anota que chegou um quadro por este portador.
    pub const fn mark_carrier_rx(&mut self, carrier: Carrier, at: Timestamp) {
        match carrier {
            Carrier::Rfcomm => self.last_rx_rfcomm = at,
            Carrier::Udp => self.last_rx_udp = at,
            Carrier::Tcp => {}
        }
    }

    /// Quando chegou o último quadro por este portador.
    #[must_use]
    pub const fn carrier_rx(&self, carrier: Carrier) -> Timestamp {
        match carrier {
            Carrier::Rfcomm => self.last_rx_rfcomm,
            Carrier::Udp | Carrier::Tcp => self.last_rx_udp,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bluetooth_wins_when_both_are_available() {
        let mut set = CarrierSet::NONE;
        set.set(Carrier::Rfcomm, true);
        set.set(Carrier::Udp, true);
        assert_eq!(
            set.pick_input_carrier(None),
            Some((Carrier::Rfcomm, CarrierChoice::Preferred))
        );
    }

    #[test]
    fn the_network_is_the_declared_fallback() {
        let mut set = CarrierSet::NONE;
        set.set(Carrier::Udp, true);
        assert_eq!(
            set.pick_input_carrier(None),
            Some((Carrier::Udp, CarrierChoice::FellBackToNetwork)),
            "e o motivo tem de vir junto, para a interface poder dizer"
        );
    }

    #[test]
    fn with_nothing_available_there_is_no_session() {
        assert_eq!(CarrierSet::NONE.pick_input_carrier(None), None);
    }

    #[test]
    fn tcp_alone_is_not_an_input_carrier() {
        let mut set = CarrierSet::NONE;
        set.set(Carrier::Tcp, true);
        assert_eq!(
            set.pick_input_carrier(None),
            None,
            "TCP nunca leva teclado e mouse"
        );
        assert!(set.has_no_input_carrier());
    }

    #[test]
    fn pinning_disables_the_fallback() {
        let mut set = CarrierSet::NONE;
        set.set(Carrier::Udp, true);
        assert_eq!(
            set.pick_input_carrier(Some(Carrier::Rfcomm)),
            None,
            "quem fixou Bluetooth excluiu a rede de propósito"
        );
        assert_eq!(
            set.pick_input_carrier(Some(Carrier::Udp)),
            Some((Carrier::Udp, CarrierChoice::PinnedByUser))
        );
    }

    #[test]
    fn pinning_tcp_is_refused_because_it_cannot_carry_input() {
        let mut set = CarrierSet::NONE;
        set.set(Carrier::Tcp, true);
        assert_eq!(set.pick_input_carrier(Some(Carrier::Tcp)), None);
    }

    #[test]
    fn losing_bluetooth_moves_the_choice_to_the_network() {
        let mut set = CarrierSet::NONE;
        set.set(Carrier::Rfcomm, true);
        set.set(Carrier::Udp, true);
        set.set(Carrier::Rfcomm, false);
        assert_eq!(
            set.pick_input_carrier(None),
            Some((Carrier::Udp, CarrierChoice::FellBackToNetwork))
        );
    }

    #[test]
    fn a_clock_started_now_has_no_expired_deadline() {
        let now = Timestamp::from_millis(50_000);
        let clock = Clock::started_at(now);
        assert_eq!(clock.last_rx, now);
        assert_eq!(clock.last_heartbeat, now);
        assert_eq!(clock.last_snapshot, now);
        assert_eq!(clock.last_pointer, now);
        assert_eq!(clock.last_bare_ack, now);
    }
}
