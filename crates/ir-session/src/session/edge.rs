//! A borda de travessia: o servidor decide, o cliente acompanha.
//!
//! Antes, cada máquina tinha a própria borda, gravada na própria configuração. No teste físico
//! (log 24) o usuário trocou dos dois lados, as duas ficaram com `left`, e a volta passou a sair
//! pelo lado errado do cliente. E trocar refazia a sessão inteira, com um adeus que ainda mandava o
//! par não reconectar.
//!
//! Agora a borda é do servidor, que é quem tem o teclado e o mouse. Ele a anuncia ao estabelecer a
//! sessão e a cada troca ([`Control::EdgeConfig`]); o cliente usa sempre a oposta; e a troca ajusta
//! a sessão em uso, sem derrubá-la.

use ir_proto::message::{Control, Message};
use ir_proto::screens::Edge;

use crate::config::Role;
use crate::event::{Command, CommandBatch, Notice};
use crate::phase::Phase;
use crate::session::Session;
use crate::time::Timestamp;

impl Session {
    /// O usuário escolheu a borda. Só o servidor escolhe; no cliente, o pedido não muda nada.
    pub(super) fn on_set_peer_edge(&mut self, now: Timestamp, edge: Edge, out: &mut CommandBatch) {
        if self.config.role != Role::Server || edge == self.config.peer_edge {
            return;
        }
        if self.phase == Phase::Engaged {
            // O controle está do outro lado, e a volta dele depende da borda. Devolver antes, pelo
            // caminho da volta normal: o cliente solta tudo e para de injetar, e só então a borda
            // muda. Os dois avisos vão pelo canal de controle, que entrega na ordem.
            let message = Control::LeaveScreen {
                leaving_edge: self.config.peer_edge.opposite(),
                position: self.local_position(),
            };
            self.send(now, Message::Control(message), out);
            self.take_control_back(u16::MAX / 2, out);
        }
        self.config.peer_edge = edge;
        out.push(Command::Notify(Notice::EdgeChanged { edge }));
        self.announce_edge(now, out);
    }

    /// Conta ao par qual é a borda, se há sessão e se esta ponta é quem decide.
    pub(super) fn announce_edge(&mut self, now: Timestamp, out: &mut CommandBatch) {
        if self.config.role != Role::Server || !self.phase.is_established() {
            return;
        }
        let message = Control::EdgeConfig {
            peer_edge: self.config.peer_edge,
        };
        self.send(now, Message::Control(message), out);
    }

    /// O servidor anunciou a borda dele, e o cliente passa a usar a oposta.
    ///
    /// No servidor o anúncio é ignorado: um cliente de outra versão, ou mal-intencionado, não
    /// decide por onde o teclado sai.
    pub(super) fn on_edge_config(&mut self, peer_edge: Edge, out: &mut CommandBatch) {
        if self.config.role != Role::Client {
            return;
        }
        let edge = peer_edge.opposite();
        if edge == self.config.peer_edge {
            return;
        }
        if self.phase == Phase::Engaged {
            // O servidor devolve o controle antes de trocar, então isto não deveria acontecer. Mas
            // a volta depende da borda, e com ela mudando o seguro é soltar tudo agora.
            self.on_leave_screen(out);
        }
        self.config.peer_edge = edge;
        out.push(Command::Notify(Notice::EdgeChanged { edge }));
    }
}
