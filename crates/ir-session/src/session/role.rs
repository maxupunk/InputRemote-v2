//! Os dois papéis combinados: cada ponta anuncia o seu, e numa colisão uma delas cede.
//!
//! Cada computador escolhe o papel na própria tela, e nada garantia que os dois combinassem. Trocar
//! num e esquecer o outro deixava dois com o teclado, brigando pela borda, ou dois controlados,
//! parados. Agora cada ponta anuncia o papel ao estabelecer; se os dois forem iguais, vale a escolha
//! mais recente, e quem perde passa ao papel complementar pela periferia ([`Notice::AdoptRole`]).
//!
//! A decisão é a mesma nas duas pontas — a mesma comparação, com os mesmos dois valores trocados de
//! lugar —, então exatamente uma cede. No empate de horário (dois papéis que nunca foram escolhidos
//! pela tela, os dois em `0`), desempata o identificador da instalação.

use ir_proto::ids::MachineId;
use ir_proto::message::{Control, Message, PeerRole};
use ir_proto::version::ROLE_CLAIM;

use crate::config::Role;
use crate::event::{Command, CommandBatch, Notice};
use crate::session::Session;
use crate::time::Timestamp;

impl Session {
    /// Conta ao par o papel daqui, se ele entende.
    pub(super) fn announce_role(&mut self, now: Timestamp, out: &mut CommandBatch) {
        let fala = self.phase.is_established()
            && self
                .peer
                .as_ref()
                .is_some_and(|peer| peer.version >= ROLE_CLAIM);
        if fala {
            let claim = Control::Role {
                role: no_protocolo(self.config.role),
                chosen_at: self.config.role_chosen_at,
            };
            self.send(now, Message::Control(claim), out);
        }
    }

    /// O par anunciou o papel dele. Se colidir com o daqui e a escolha dele for a que vale, a
    /// periferia passa esta ponta ao complementar.
    pub(super) fn on_peer_role(&self, role: PeerRole, chosen_at: u64, out: &mut CommandBatch) {
        let aqui = no_protocolo(self.config.role);
        let Some(peer) = self.peer.as_ref() else {
            return;
        };
        if role != aqui {
            return;
        }
        let dele = (chosen_at, &peer.machine);
        let meu = (self.config.role_chosen_at, &self.identity.machine);
        if vence(dele, meu) {
            out.push(Command::Notify(Notice::AdoptRole {
                role: da_sessao(aqui.complement()),
                chosen_at,
            }));
        }
    }
}

/// Se a escolha `a` vale sobre a `b`: a mais recente; no empate, a do identificador menor.
fn vence(a: (u64, &MachineId), b: (u64, &MachineId)) -> bool {
    a.0 > b.0 || (a.0 == b.0 && a.1.0 < b.1.0)
}

const fn no_protocolo(role: Role) -> PeerRole {
    match role {
        Role::Server => PeerRole::Server,
        Role::Client => PeerRole::Client,
    }
}

const fn da_sessao(role: PeerRole) -> Role {
    match role {
        PeerRole::Server => Role::Server,
        PeerRole::Client => Role::Client,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exatamente_uma_ponta_cede() {
        let (a, b) = (MachineId([1; 16]), MachineId([2; 16]));
        for (ta, tb) in [(0, 0), (5, 3), (3, 5), (7, 7)] {
            let a_cede = vence((tb, &b), (ta, &a));
            let b_cede = vence((ta, &a), (tb, &b));
            assert_ne!(a_cede, b_cede, "horários {ta} e {tb}");
        }
    }

    #[test]
    fn vale_a_escolha_mais_recente() {
        let (a, b) = (MachineId([1; 16]), MachineId([2; 16]));
        assert!(
            vence((10, &b), (5, &a)),
            "a mais recente vale mesmo com id maior"
        );
        assert!(!vence((5, &a), (10, &b)));
    }
}
