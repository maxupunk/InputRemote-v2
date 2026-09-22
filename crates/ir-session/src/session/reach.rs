//! O alcance: contar ao par por onde mais esta máquina pode ser alcançada.
//!
//! A rota dupla precisa de um endereço em cada portador. O da rede a descoberta acha sozinha, a
//! partir do `MachineId` do par; o do rádio não tem como ser achado pela rede — a não ser que o par
//! conte. É o que [`Control::Reach`] faz: cada lado anuncia o próprio rádio ao estabelecer a
//! sessão, e o outro passa a poder discar o Bluetooth mesmo quando os dois se conheceram pela rede
//! ([ADR-0012](../../../../docs/adr/0012-rota-dupla.md)).
//!
//! A sessão não disca nada: ela anuncia o que é daqui e repassa o que ouviu do par. Discar é da
//! periferia, que é quem tem os transportes.

use ir_proto::ids::RadioAddress;
use ir_proto::message::{Control, Message};

use crate::event::{Command, CommandBatch, Notice};
use crate::session::Session;
use crate::time::Timestamp;

impl Session {
    /// O endereço do rádio desta máquina ficou conhecido, ou mudou.
    pub(super) fn on_local_radio(
        &mut self,
        now: Timestamp,
        radio: RadioAddress,
        out: &mut CommandBatch,
    ) {
        if self.local_radio == Some(radio) {
            return;
        }
        self.local_radio = Some(radio);
        if self.phase.is_established() {
            self.announce_reach(now, out);
        }
    }

    /// Conta ao par o endereço do rádio daqui, se ele for conhecido.
    pub(super) fn announce_reach(&mut self, now: Timestamp, out: &mut CommandBatch) {
        if let Some(radio) = self.local_radio {
            self.send(now, Message::Control(Control::Reach { radio }), out);
        }
    }

    /// O par contou o endereço do rádio dele.
    pub(super) fn on_reach(radio: RadioAddress, out: &mut CommandBatch) {
        out.push(Command::Notify(Notice::PeerRadio(radio)));
    }
}
