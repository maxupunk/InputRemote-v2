//! Para onde a entrada vai: quem mexe por último, manda.
//!
//! Sem papel fixo ([ADR-0014](../../../docs/adr/0014-controle-simetrico.md)), três regras decidem a
//! direção, e as três moram aqui:
//!
//! 1. **Retomar.** Com o par usando esta tela ([`Phase::Receiving`]), mexer no teclado ou no mouse
//!    daqui devolve o controle a esta máquina na hora ([`Control::Reclaim`]). Duas pessoas, cada uma
//!    na sua mesa, usam os dois computadores sem combinar nada.
//! 2. **Contra tremida.** Um esbarrão na mesa não retoma: vale um clique, uma tecla, a roda, ou o
//!    ponteiro andar [`RECLAIM_DISTANCE`] dentro de [`Timings::reclaim_window`]. E logo depois de o
//!    par chegar ([`Timings::reclaim_grace`]) nada retoma — é a mão dele ainda chegando.
//! 3. **Os dois atravessam juntos.** Cada um manda `EnterScreen` ao outro. Cede quem tem o
//!    identificador maior, e passa a receber; a comparação é a mesma dos dois lados, então sempre
//!    exatamente um cede.
//!
//! [`Timings::reclaim_window`]: crate::config::Timings::reclaim_window
//! [`Timings::reclaim_grace`]: crate::config::Timings::reclaim_grace

use ir_proto::message::{Control, Message};

use crate::event::{Command, CommandBatch, Notice};
use crate::phase::Phase;
use crate::session::Session;
use crate::time::Timestamp;

/// Quanto o ponteiro daqui precisa andar, em pixels, para retomar o controle.
pub const RECLAIM_DISTANCE: i32 = 20;

/// O que se acompanha enquanto o par usa esta tela, para decidir se o daqui quer retomar.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct ReclaimWatch {
    /// Quando o par chegou.
    since: Timestamp,
    /// Quando começou a janela em que o ponteiro daqui está sendo somado.
    window_start: Timestamp,
    /// Quanto o ponteiro daqui andou nesta janela.
    travelled: i32,
}

impl Session {
    /// Se a entrada daqui pode atravessar agora: a política deixa, e o par aceita ser controlado.
    pub(super) fn may_cross(&self) -> bool {
        self.config.policy.sends()
            && self
                .peer
                .as_ref()
                .is_some_and(|peer| !peer.capabilities.declines_control)
    }

    /// O par passou a usar esta tela.
    pub(super) fn start_receiving(&mut self, now: Timestamp) {
        self.reclaim_watch = ReclaimWatch {
            since: now,
            window_start: now,
            travelled: 0,
        };
    }

    /// Se o par acabou de chegar, e o que se mexer aqui ainda não conta.
    fn in_grace(&self, now: Timestamp) -> bool {
        !now.elapsed_at_least(self.reclaim_watch.since, self.config.timings.reclaim_grace)
    }

    /// O ponteiro daqui andou enquanto o par usa esta tela.
    pub(super) fn local_motion_while_receiving(
        &mut self,
        now: Timestamp,
        dx: i32,
        dy: i32,
        out: &mut CommandBatch,
    ) {
        if self.in_grace(now) {
            return;
        }
        let window = self.config.timings.reclaim_window;
        if now.elapsed_at_least(self.reclaim_watch.window_start, window) {
            self.reclaim_watch.window_start = now;
            self.reclaim_watch.travelled = 0;
        }
        let passo = dx.saturating_abs().saturating_add(dy.saturating_abs());
        self.reclaim_watch.travelled = self.reclaim_watch.travelled.saturating_add(passo);
        if self.reclaim_watch.travelled >= RECLAIM_DISTANCE {
            self.reclaim(now, out);
        }
    }

    /// Uma tecla, um botão ou a roda daqui, enquanto o par usa esta tela: é gesto, retoma.
    ///
    /// Só a descida conta: a subida de uma tecla que estava apertada antes de o par chegar não é
    /// alguém querendo usar esta máquina.
    pub(super) fn local_press_while_receiving(
        &mut self,
        now: Timestamp,
        pressed: bool,
        out: &mut CommandBatch,
    ) {
        if pressed && !self.in_grace(now) {
            self.reclaim(now, out);
        }
    }

    /// Esta máquina retoma o controle: solta o que o par tinha apertado aqui, e avisa.
    ///
    /// O que o daqui digitou ou clicou para retomar já chegou ao sistema: a entrada local não é
    /// suprimida enquanto se recebe.
    pub(super) fn reclaim(&mut self, now: Timestamp, out: &mut CommandBatch) {
        if self.phase != Phase::Receiving {
            return;
        }
        self.release_everything(out);
        self.phase = Phase::Ready;
        self.send(now, Message::Control(Control::Reclaim), out);
        out.push(Command::Notify(Notice::ControlReclaimed { here: true }));
    }

    /// O par retomou o controle, ou recusou recebê-lo: a entrada daqui volta a ser daqui.
    ///
    /// O cursor daqui fica onde saiu, encostado na borda. Nada de voltar pela borda: a volta é
    /// deste lado, e o par nem sabe onde o cursor dele estava.
    pub(super) fn on_peer_reclaim(&mut self, out: &mut CommandBatch) {
        if self.phase != Phase::Sending {
            return;
        }
        self.hand_control_back(out);
        out.push(Command::Notify(Notice::ControlReclaimed { here: false }));
    }

    /// Os dois atravessaram ao mesmo tempo, e o par entregou o controle a esta máquina enquanto
    /// ela o entregava a ele. Devolve se esta ponta cede — e, cedendo, para de mandar.
    pub(super) fn yield_crossing(&mut self, out: &mut CommandBatch) -> bool {
        let Some(peer) = self.peer.as_ref() else {
            return false;
        };
        if self.identity.machine.0 < peer.machine.0 {
            return false; // o par faz a mesma conta, e cede ele
        }
        // O que foi mandado ao par o par ignora, porque ele está mandando; aqui, a supressão cai.
        self.input_state.release_all();
        self.pending_pointer = ir_proto::input::PointerDelta::ZERO;
        out.push(Command::SuppressLocalInput(false));
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_reclaim_distance_is_a_gesture_and_not_a_tremor() {
        // Um mouse de 1000 DPI anda ~20 contagens em meio milímetro de mão: um esbarrão fica
        // abaixo; qualquer gesto de ir usar esta tela passa.
        assert!((10..=40).contains(&RECLAIM_DISTANCE));
    }
}
