//! A rota: por quais portadores de entrada a sessão fala, e como ela muda sem refazer a sessão.
//!
//! Com um portador só, a rota é ele. Com os dois de pé e nada fixado, a rota é **dupla**: cada
//! quadro sai pelo Bluetooth **e** pela rede, e do outro lado vale o que chegar primeiro — a cópia
//! que chega depois é descartada pela mesma detecção de repetição que já protege o UDP
//! ([ADR-0012](../../../../docs/adr/0012-rota-dupla.md)).
//!
//! # Por que a rota dupla é segura
//!
//! Dois caminhos juntos se comportam como um datagrama: podem duplicar e reordenar, e cada um pode
//! perder o que estava nele quando caiu. Essa é exatamente a garantia que a sessão já trata sobre
//! UDP — número de sequência por canal, confirmação, retransmissão, fila de reordenação e descarte
//! de repetição. Por isso, desde a versão 2 do protocolo, a sessão trata **todo** portador de
//! entrada como datagrama, inclusive o RFCOMM sozinho: a garantia da sessão não muda quando um
//! portador entra ou sai da rota, e é isso que permite a troca sem aperto de mão.
//!
//! # O que a rota muda, e o que não muda
//!
//! Um portador entrar ou sair da rota **não** derruba a sessão, **não** solta teclas e **não**
//! começa encarnação nova: o que estava em trânsito no portador que caiu chega pelo outro, ou pela
//! retransmissão. Só a queda do **último** portador encerra a sessão — e aí vale a regra de
//! sempre, soltar tudo antes de qualquer outra coisa ([`link`](super::link)).

use ir_proto::carrier::Carrier;
use ir_proto::frame::Frame;

use crate::event::{CarrierChoice, Command, CommandBatch, Notice};
use crate::session::Session;

/// Por quais portadores de entrada a sessão fala.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Route {
    /// Um portador só.
    Single(Carrier),
    /// Bluetooth e rede ao mesmo tempo; vale o que chegar primeiro.
    Dual,
}

impl Route {
    /// Os dois portadores de entrada, na ordem de preferência.
    const INPUT: [Carrier; 2] = [Carrier::Rfcomm, Carrier::Udp];

    /// O portador que representa a rota numa frase só.
    ///
    /// Na rota dupla é o Bluetooth, o preferido da política única: quem precisa de **um** nome —
    /// o aviso de conexão, o estado antigo da interface — continua recebendo o mesmo de antes.
    #[must_use]
    pub const fn primary(self) -> Carrier {
        match self {
            Self::Single(carrier) => carrier,
            Self::Dual => Carrier::Rfcomm,
        }
    }

    /// Se a rota passa por este portador.
    #[must_use]
    pub const fn uses(self, carrier: Carrier) -> bool {
        match self {
            Self::Single(own) => matches!(
                (own, carrier),
                (Carrier::Rfcomm, Carrier::Rfcomm)
                    | (Carrier::Udp, Carrier::Udp)
                    | (Carrier::Tcp, Carrier::Tcp)
            ),
            Self::Dual => carrier.carries_input(),
        }
    }

    /// Se a rota é dupla.
    #[must_use]
    pub const fn is_dual(self) -> bool {
        matches!(self, Self::Dual)
    }

    /// Os portadores da rota, na ordem de preferência.
    pub fn carriers(self) -> impl Iterator<Item = Carrier> {
        Self::INPUT
            .into_iter()
            .filter(move |carrier| self.uses(*carrier))
    }

    /// A rota com este portador a mais.
    #[must_use]
    pub const fn with(self, carrier: Carrier) -> Self {
        if self.uses(carrier) || !carrier.carries_input() {
            self
        } else {
            Self::Dual
        }
    }

    /// A rota sem este portador, ou nada se ele era o último.
    #[must_use]
    pub const fn without(self, carrier: Carrier) -> Option<Self> {
        match self {
            Self::Dual => match carrier {
                Carrier::Rfcomm => Some(Self::Single(Carrier::Udp)),
                Carrier::Udp => Some(Self::Single(Carrier::Rfcomm)),
                Carrier::Tcp => Some(Self::Dual),
            },
            Self::Single(_) if self.uses(carrier) => None,
            Self::Single(_) => Some(self),
        }
    }
}

/// Por qual portador cada quadro novo chegou primeiro.
///
/// É o placar da rota dupla, e o dado que responde "vale a pena?": se a rede vence quase sempre,
/// o Bluetooth está ali de reserva; se as vitórias se dividem, cada um está cobrindo os picos do
/// outro. Conta só quadros **novos** — a cópia que chega depois já perdeu, e é descartada.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CarrierWins {
    /// Quantos quadros novos chegaram primeiro pelo Bluetooth.
    pub rfcomm: u64,
    /// Quantos chegaram primeiro pela rede.
    pub udp: u64,
}

impl CarrierWins {
    /// Conta uma vitória deste portador.
    pub(super) const fn count(&mut self, carrier: Carrier) {
        match carrier {
            Carrier::Rfcomm => self.rfcomm = self.rfcomm.saturating_add(1),
            Carrier::Udp => self.udp = self.udp.saturating_add(1),
            Carrier::Tcp => {}
        }
    }
}

/// Um retrato da rota, para o registro e o diagnóstico: por onde, quem chegou primeiro, e há quanto
/// tempo não se ouve cada portador da rota.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RouteReport {
    /// A rota em uso, se há sessão.
    pub route: Option<Route>,
    /// O placar desde que a sessão subiu.
    pub wins: CarrierWins,
    /// Há quanto tempo não chega nada pelo Bluetooth, se ele está na rota.
    pub silence_rfcomm: Option<crate::time::Millis>,
    /// Há quanto tempo não chega nada pela rede, se ela está na rota.
    pub silence_udp: Option<crate::time::Millis>,
}

impl core::fmt::Display for RouteReport {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self.route {
            Some(route) => write!(f, "{route}")?,
            None => f.write_str("nenhuma")?,
        }
        write!(
            f,
            "; primeiro a chegar: Bluetooth {}, rede {}",
            self.wins.rfcomm, self.wins.udp
        )?;
        for (nome, silencio) in [
            ("Bluetooth", self.silence_rfcomm),
            ("rede", self.silence_udp),
        ] {
            if let Some(silencio) = silencio {
                write!(f, "; {nome} ouvido há {silencio}")?;
            }
        }
        Ok(())
    }
}

impl core::fmt::Display for Route {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Single(carrier) => write!(f, "{carrier}"),
            Self::Dual => f.write_str("bluetooth+udp"),
        }
    }
}

impl Session {
    /// Despacha um quadro por todos os portadores da rota.
    ///
    /// É o único lugar onde um quadro vira `Command::Send`, e por isso é o único lugar que sabe
    /// que um quadro pode sair duas vezes. Quem monta o quadro não precisa saber.
    pub(super) fn dispatch_on_route(&self, frame: Frame, out: &mut CommandBatch) {
        let Some(route) = self.route else { return };
        let mut carriers = route.carriers();
        let Some(mut current) = carriers.next() else {
            return;
        };
        for next in carriers {
            out.push(Command::Send {
                carrier: current,
                frame: frame.clone(),
            });
            current = next;
        }
        // O último portador leva o original: a rota simples não paga cópia nenhuma.
        out.push(Command::Send {
            carrier: current,
            frame,
        });
    }

    /// Se a sessão pode juntar portadores numa rota dupla.
    ///
    /// Fixar um portador desliga a redundância pelo mesmo motivo que desliga a degradação: quem
    /// fixou excluiu o outro de propósito.
    pub(super) const fn may_widen_route(&self) -> bool {
        self.pinned.is_none() && self.phase.is_established()
    }

    /// Acrescenta um portador à rota da sessão já de pé — sem aperto de mão, sem soltar nada.
    ///
    /// Devolve se a rota mudou.
    pub(super) fn widen_route(&mut self, carrier: Carrier, out: &mut CommandBatch) -> bool {
        if !self.may_widen_route() || !self.available.has(carrier) || !carrier.carries_input() {
            return false;
        }
        let Some(route) = self.route else {
            return false;
        };
        let widened = route.with(carrier);
        if widened == route {
            return false;
        }
        self.route = Some(widened);
        self.clock.mark_carrier_rx(carrier, self.clock.last_rx);
        out.push(Command::Notify(Notice::RouteChanged {
            route: widened,
            why: CarrierChoice::Redundant,
        }));
        true
    }

    /// Junta à rota todo portador de entrada disponível. Chamado quando a sessão fica de pé.
    pub(super) fn widen_route_to_available(&mut self, out: &mut CommandBatch) {
        for carrier in Route::INPUT {
            self.widen_route(carrier, out);
        }
    }

    /// Tira um portador da rota dupla. Devolve `false` quando ele era o último — e aí quem
    /// chamou precisa encerrar a sessão.
    pub(super) fn narrow_route(&mut self, carrier: Carrier, out: &mut CommandBatch) -> bool {
        let Some(route) = self.route else {
            return false;
        };
        match route.without(carrier) {
            None => false,
            Some(narrowed) if narrowed == route => true,
            Some(narrowed) => {
                self.route = Some(narrowed);
                let why = self
                    .available
                    .pick_input_carrier(self.pinned)
                    .map_or(CarrierChoice::Preferred, |(_, why)| why);
                out.push(Command::Notify(Notice::RouteChanged {
                    route: narrowed,
                    why,
                }));
                true
            }
        }
    }

    /// Fixa ou solta um portador com a sessão de pé.
    ///
    /// Fixar um portador que está na rota dupla estreita a rota para ele, sem refazer a sessão; o
    /// outro continua disponível, só deixa de ser usado. Soltar a fixação volta a juntar tudo.
    pub(super) fn apply_pin_to_route(&mut self, out: &mut CommandBatch) {
        let Some(route) = self.route else { return };
        match self.pinned {
            Some(pinned) if route.is_dual() && route.uses(pinned) => {
                let other = route.carriers().find(|carrier| *carrier != pinned);
                if let Some(other) = other {
                    self.route = route.without(other);
                    out.push(Command::Notify(Notice::RouteChanged {
                        route: Route::Single(pinned),
                        why: CarrierChoice::PinnedByUser,
                    }));
                }
            }
            Some(_) => {}
            None => self.widen_route_to_available(out),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_dual_route_uses_both_input_carriers_and_never_tcp() {
        assert!(Route::Dual.uses(Carrier::Rfcomm));
        assert!(Route::Dual.uses(Carrier::Udp));
        assert!(!Route::Dual.uses(Carrier::Tcp));
        assert_eq!(
            Route::Dual.carriers().collect::<Vec<_>>(),
            vec![Carrier::Rfcomm, Carrier::Udp]
        );
    }

    #[test]
    fn a_single_route_uses_only_its_carrier() {
        let route = Route::Single(Carrier::Udp);
        assert_eq!(route.carriers().collect::<Vec<_>>(), vec![Carrier::Udp]);
        assert!(!route.uses(Carrier::Rfcomm));
    }

    #[test]
    fn widening_adds_only_input_carriers() {
        assert_eq!(
            Route::Single(Carrier::Udp).with(Carrier::Rfcomm),
            Route::Dual
        );
        assert_eq!(
            Route::Single(Carrier::Udp).with(Carrier::Tcp),
            Route::Single(Carrier::Udp),
            "arquivos nunca entram na rota da entrada"
        );
        assert_eq!(Route::Dual.with(Carrier::Udp), Route::Dual);
    }

    #[test]
    fn narrowing_keeps_the_other_carrier_and_ends_on_the_last() {
        assert_eq!(
            Route::Dual.without(Carrier::Udp),
            Some(Route::Single(Carrier::Rfcomm))
        );
        assert_eq!(
            Route::Dual.without(Carrier::Rfcomm),
            Some(Route::Single(Carrier::Udp))
        );
        assert_eq!(
            Route::Single(Carrier::Rfcomm).without(Carrier::Rfcomm),
            None
        );
        assert_eq!(
            Route::Single(Carrier::Rfcomm).without(Carrier::Udp),
            Some(Route::Single(Carrier::Rfcomm)),
            "tirar um portador que a rota não usa não muda nada"
        );
    }

    #[test]
    fn the_report_says_the_route_the_tally_and_the_silence() {
        let report = RouteReport {
            route: Some(Route::Dual),
            wins: CarrierWins { rfcomm: 3, udp: 97 },
            silence_rfcomm: Some(crate::time::Millis(4000)),
            silence_udp: Some(crate::time::Millis(5)),
        };
        let texto = report.to_string();
        assert!(texto.starts_with("bluetooth+udp"), "{texto}");
        assert!(texto.contains("Bluetooth 3, rede 97"), "{texto}");
        assert!(texto.contains("Bluetooth ouvido há"), "{texto}");
    }

    #[test]
    fn the_dual_route_is_represented_by_the_preferred_carrier() {
        assert_eq!(Route::Dual.primary(), Carrier::Rfcomm);
        assert_eq!(Route::Single(Carrier::Udp).primary(), Carrier::Udp);
    }
}
