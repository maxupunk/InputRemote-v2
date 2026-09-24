//! A borda de travessia: de qualquer lado, a escolha mais recente vale.
//!
//! "O Fedora fica à direita" num computador é "o Windows fica à esquerda" no outro: as duas bordas
//! são sempre opostas. Antes a borda era do servidor, e o cliente só acompanhava. Sem papel fixo
//! ([ADR-0014](../../../docs/adr/0014-controle-simetrico.md)), qualquer um dos dois muda a posição
//! na própria tela, e o outro passa a usar a oposta.
//!
//! Cada ponta anuncia a borda dela com o horário em que foi escolhida ([`Control::EdgeConfig`]), ao
//! estabelecer e a cada troca. Se as duas não forem opostas, vale a mais recente; no empate — duas
//! bordas nunca escolhidas pela tela —, a do menor identificador de instalação. A comparação é a
//! mesma nas duas pontas com os valores trocados de lugar, então exatamente uma cede.

use ir_proto::ids::MachineId;
use ir_proto::message::{Control, Message};
use ir_proto::screens::Edge;

use crate::event::{Command, CommandBatch, Notice};
use crate::phase::Phase;
use crate::session::Session;
use crate::time::Timestamp;

impl Session {
    /// Começa o ponteiro encostado no lado **oposto** à borda de travessia, no meio dele.
    ///
    /// Para quem não sabe onde o cursor real está e não o conduz — o Linux sem injetor. Semeado no
    /// meio, o modelo podia estar à frente do cursor e atravessar com ele longe da borda; do lado
    /// oposto, o erro cai sempre para o lado seguro (log 49).
    pub fn seed_pointer_away_from_edge(&mut self) {
        let Some(desktop) = self.local_screens.as_ref() else {
            return;
        };
        let longe = desktop
            .bounds()
            .point_along(self.config.peer_edge.opposite(), u16::MAX / 2);
        self.pointer = desktop.nearest_valid(longe);
    }

    /// O usuário escolheu, nesta tela, de que lado fica o par.
    pub(super) fn on_set_peer_edge(
        &mut self,
        now: Timestamp,
        edge: Edge,
        chosen_at: u64,
        out: &mut CommandBatch,
    ) {
        if edge == self.config.peer_edge {
            return;
        }
        self.settle_before_edge_change(now, out);
        self.config.peer_edge = edge;
        self.config.edge_chosen_at = chosen_at;
        out.push(Command::Notify(Notice::EdgeChanged { edge, chosen_at }));
        self.announce_edge(now, out);
    }

    /// Conta ao par qual é a borda daqui, e desde quando.
    pub(super) fn announce_edge(&mut self, now: Timestamp, out: &mut CommandBatch) {
        if !self.phase.is_established() {
            return;
        }
        let message = Control::EdgeConfig {
            peer_edge: self.config.peer_edge,
            chosen_at: self.config.edge_chosen_at,
        };
        self.send(now, Message::Control(message), out);
    }

    /// O par anunciou a borda dele. Se ela não for a oposta da daqui e a escolha dele for a que
    /// vale, esta ponta passa a usar a oposta da dele.
    pub(super) fn on_edge_config(
        &mut self,
        now: Timestamp,
        peer_edge: Edge,
        chosen_at: u64,
        out: &mut CommandBatch,
    ) {
        let edge = peer_edge.opposite();
        if edge == self.config.peer_edge {
            return;
        }
        let Some(peer) = self.peer.as_ref() else {
            return;
        };
        let dele = (chosen_at, &peer.machine);
        let minha = (self.config.edge_chosen_at, &self.identity.machine);
        if !prevails(dele, minha) {
            return; // o par faz a mesma conta com o anúncio daqui, e cede
        }
        self.settle_before_edge_change(now, out);
        self.config.peer_edge = edge;
        // O horário do par, e não o de agora: senão esta ponta venceria a próxima comparação e o
        // par cederia de volta.
        self.config.edge_chosen_at = chosen_at;
        out.push(Command::Notify(Notice::EdgeAdopted { edge, chosen_at }));
    }

    /// Antes de a borda mudar, o controle volta a cada um: a volta dele depende da borda.
    fn settle_before_edge_change(&mut self, now: Timestamp, out: &mut CommandBatch) {
        match self.phase {
            // Devolver pelo caminho da volta normal: o par solta tudo e para de injetar, e só
            // então a borda muda. Os dois avisos vão pelo canal de controle, que entrega na ordem.
            Phase::Sending => {
                let message = Control::LeaveScreen {
                    leaving_edge: self.config.peer_edge.opposite(),
                    position: self.local_position(),
                };
                self.send(now, Message::Control(message), out);
                self.take_control_back(u16::MAX / 2, out);
            }
            Phase::Receiving => self.reclaim(now, out),
            _ => {}
        }
    }
}

/// Se a escolha `a` vale sobre a `b`: a mais recente; no empate, a do identificador menor.
pub(super) fn prevails(a: (u64, &MachineId), b: (u64, &MachineId)) -> bool {
    a.0 > b.0 || (a.0 == b.0 && a.1.0 < b.1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exactly_one_side_prevails() {
        let (a, b) = (MachineId([1; 16]), MachineId([2; 16]));
        for (ta, tb) in [(0, 0), (5, 3), (3, 5), (7, 7)] {
            assert_ne!(
                prevails((ta, &a), (tb, &b)),
                prevails((tb, &b), (ta, &a)),
                "horários {ta} e {tb}"
            );
        }
    }

    #[test]
    fn the_latest_choice_prevails_even_with_a_larger_id() {
        let (a, b) = (MachineId([1; 16]), MachineId([2; 16]));
        assert!(prevails((10, &b), (5, &a)));
        assert!(!prevails((5, &a), (10, &b)));
    }
}
