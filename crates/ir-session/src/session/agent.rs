//! O que acontece do lado de cá: as telas desta máquina mudaram, o agente sumiu, e soltar tudo.
//!
//! Saiu de [`super`] por tamanho: são as entradas que vêm da própria máquina, e não do par.

use ir_geometry::Desktop;
use ir_proto::message::{Control, Message};
use ir_proto::screens::ScreenLayout;

use super::Session;
use crate::event::{Command, CommandBatch};
use crate::phase::Phase;
use crate::time::Timestamp;

impl Session {
    /// Atualiza o arranjo local e conta ao par.
    pub(super) fn on_local_screens(
        &mut self,
        now: Timestamp,
        layout: &ScreenLayout,
        out: &mut CommandBatch,
    ) {
        self.local_screens = Desktop::from_layout(layout);
        if let Some(desktop) = self.local_screens.as_ref() {
            // A posição guardada pode ter ficado fora de qualquer tela quando um monitor foi
            // removido. Trazer de volta aqui evita coordenada inválida em todo o resto.
            self.pointer = desktop.nearest_valid(self.pointer);
        }
        self.announce_screens(now, out);
    }

    /// Conta ao par o arranjo de telas daqui, se há sessão e o arranjo é conhecido.
    ///
    /// Vai o arranjo que esta máquina **usa**, e não o que chegou: um monitor que a conversão
    /// descartou não é reanunciado ao par como se valesse.
    pub(super) fn announce_screens(&mut self, now: Timestamp, out: &mut CommandBatch) {
        if !self.phase.is_established() {
            return;
        }
        if let Some(desktop) = self.local_screens.as_ref() {
            let layout = desktop.to_layout();
            self.send(now, Message::Control(Control::Screens(layout)), out);
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
        match self.phase {
            // Devolve o controle: sem agente não há como injetar, e segurar o ponteiro do
            // usuário do outro lado seria pior. É uma retomada, como a emergência deste lado: o
            // cursor do par fica onde saiu, e a volta não depende de onde estava o daqui.
            Phase::Receiving => self.reclaim(now, out),
            Phase::Sending => self.abandon_control(now, out),
            // Sem entrada atravessando não há o que soltar: o agente novo nasce limpo.
            Phase::Offline | Phase::Handshaking | Phase::Ready => {}
        }
    }

    /// Solta tudo, local e logicamente.
    pub(super) fn release_everything(&mut self, out: &mut CommandBatch) {
        self.input_state.release_all();
        out.push(Command::ReleaseAll);
    }
}
