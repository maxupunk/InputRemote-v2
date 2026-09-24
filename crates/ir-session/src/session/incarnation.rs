//! Encarnações, do lado da sessão: o que fazer quando o par começou uma sessão nova.
//!
//! A decisão de a qual encarnação um quadro pertence é de `ir_confiabilidade::incarnation`; aqui
//! fica a reação — encerrar a sessão que o par abandonou, e recomeçar junto (log 22).

use ir_confiabilidade::incarnation::Admission;
use ir_proto::carrier::Carrier;
use ir_proto::frame::{Epoch, Frame};

use crate::event::{CommandBatch, LinkDown};
use crate::phase::Phase;
use crate::session::{Clock, Session};
use crate::time::Timestamp;

impl Session {
    /// Decide se um quadro recebido pertence à sessão, e acompanha o par quando ele começou outra.
    ///
    /// Devolve `false` quando o quadro deve ser descartado sem mais nada: nem prova de vida, nem
    /// confirmação, nem adoção de portador.
    pub(super) fn admit(
        &mut self,
        now: Timestamp,
        carrier: Carrier,
        frame: &Frame,
        out: &mut CommandBatch,
    ) -> bool {
        match self.incarnations.admit(frame) {
            Admission::Current => true,
            Admission::Stale => false,
            Admission::NewPeer => {
                self.follow_new_peer(now, carrier, frame.epoch, out);
                true
            }
        }
    }

    fn follow_new_peer(
        &mut self,
        now: Timestamp,
        carrier: Carrier,
        epoch: Epoch,
        out: &mut CommandBatch,
    ) {
        if self.phase.is_established() {
            // O par reiniciou sem que soubéssemos — o adeus dele se perdeu. A sessão daqui é de
            // uma encarnação que ele já abandonou: encerrá-la, soltando tudo, e recomeçar junto.
            self.tear_down(now, LinkDown::PeerRestarted, out);
        }
        if self.phase == Phase::Offline {
            if carrier.carries_input() {
                // Quem ouve primeiro, responde — mas numa encarnação própria, do zero. Sem isto a
                // resposta sairia com a numeração e a época de uma sessão que já acabou.
                self.available.set(carrier, true);
                self.route = Some(super::Route::Single(carrier));
                self.last_pointer_rx = None;
                self.seqs.reset();
                self.reliability.reset();
                self.clock = Clock::started_at(now);
                self.incarnations.start_local();
                self.move_to(Phase::Handshaking, out);
            }
        } else {
            // No meio do nosso aperto de mão: o que o par mandou até aqui era de outra sessão dele.
            self.reliability.reset_receivers();
            self.last_pointer_rx = None;
        }
        self.incarnations.follow_peer(epoch);
    }
}
