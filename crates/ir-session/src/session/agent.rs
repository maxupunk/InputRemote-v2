//! O que acontece do lado de cá: as telas desta máquina mudaram, o agente sumiu, e soltar tudo.
//!
//! Saiu de [`super`] por tamanho: são as entradas que vêm da própria máquina, e não do par.

use ir_geometry::Desktop;
use ir_proto::message::Message;
use ir_proto::screens::ScreenLayout;

use super::Session;
use crate::config::Role;
use crate::event::{Command, CommandBatch};
use crate::time::Timestamp;

impl Session {
    /// Atualiza o arranjo local e conta ao par.
    pub(super) fn on_local_screens(
        &mut self,
        now: Timestamp,
        layout: ScreenLayout,
        out: &mut CommandBatch,
    ) {
        self.local_screens = Desktop::from_layout(&layout);
        if let Some(desktop) = self.local_screens.as_ref() {
            // A posição guardada pode ter ficado fora de qualquer tela quando um monitor foi
            // removido. Trazer de volta aqui evita coordenada inválida em todo o resto.
            self.pointer = desktop.nearest_valid(self.pointer);
        }
        if self.phase.is_established() {
            self.send(
                now,
                Message::Control(ir_proto::message::Control::Screens(layout)),
                out,
            );
        }
    }

    /// O agente local sumiu.
    ///
    /// Se havia entrada em curso, solta tudo antes de qualquer outra coisa: o agente novo vai
    /// nascer sem saber o que estava pressionado, e o estado é do serviço exatamente para
    /// isto (`docs/02-arquitetura.md` §1.1).
    ///
    /// Do lado que controla, o agente era quem capturava: as teclas que ele mandou descer no par
    /// nunca vão ter a subida. O par é mandado soltar tudo, e o controle volta para cá — o mesmo
    /// que o atalho de emergência faz.
    pub(super) fn on_agent_lost(&mut self, now: Timestamp, out: &mut CommandBatch) {
        self.agent_ready = false;
        if !self.phase.may_hold_input() {
            return;
        }
        self.release_everything(out);
        match self.config.role {
            // Devolve o controle: sem agente não há como injetar, e segurar o ponteiro do
            // usuário do outro lado seria pior.
            Role::Client => self.report_edge_return(now, out),
            Role::Server => {
                self.send(
                    now,
                    Message::Input(ir_proto::message::InputMessage::ReleaseAll),
                    out,
                );
                self.hand_control_back(out);
            }
        }
    }

    /// Solta tudo, local e logicamente.
    pub(super) fn release_everything(&mut self, out: &mut CommandBatch) {
        self.input_state.release_all();
        out.push(Command::ReleaseAll);
    }
}
