//! O lado que tem o teclado: captura, travessia e envio.
//!
//! Duas assimetrias importantes com relação ao cliente:
//!
//! - o servidor **nunca injeta**; ele só captura e envia;
//! - enquanto o controle está no par, o servidor não sabe onde o ponteiro remoto está. Ele
//!   manda deltas crus, e quem converte para posição absoluta é o cliente, que é quem conhece
//!   o próprio arranjo de telas. Isso evita que os dois lados mantenham a mesma coordenada e
//!   discordem dela.

use ir_geometry::{Crossing, Movement, advance};
use ir_proto::ids::MonitorId;
use ir_proto::input::{Button, HidUsage, PointerDelta, PointerPosition, WheelDelta};
use ir_proto::message::{Control, InputMessage, Message, PointerMessage};
use ir_proto::screens::Edge;

use crate::config::Role;
use crate::event::{Command, CommandBatch, Notice};
use crate::phase::Phase;
use crate::session::Session;
use crate::time::Timestamp;

impl Session {
    pub(super) fn on_local_pointer(
        &mut self,
        now: Timestamp,
        delta: PointerDelta,
        out: &mut CommandBatch,
    ) {
        if self.config.role != Role::Server || !self.phase.is_established() {
            return;
        }

        if self.phase == Phase::Engaged {
            // Coalescer e despachar no intervalo: perder amostra intermediária é invisível,
            // atrasar não é (`docs/02-arquitetura.md` §6, regra 5).
            self.pending_pointer = self.pending_pointer.coalesced_with(delta);
            self.flush_pointer_if_due(now, out);
            return;
        }

        let Some(desktop) = self.local_screens.as_ref() else {
            return; // sem arranjo conhecido não há borda para atravessar
        };

        match advance(desktop, self.pointer, delta, self.config.peer_edge) {
            Movement::Stayed(point) => self.pointer = point,
            // Com a borda travada, bater nela não atravessa: o ponteiro fica onde está.
            Movement::Crossed(_) if self.edge_locked => {}
            Movement::Crossed(crossing) => self.give_control_away(now, crossing, out),
        }
    }

    /// Leva o controle ao par sem passar pela borda: pelo meio dela, como se tivesse atravessado ali.
    pub(super) fn switch_to_peer(&mut self, now: Timestamp, out: &mut CommandBatch) {
        let crossing = Crossing {
            exit_edge: self.config.peer_edge,
            fraction: u16::MAX / 2,
        };
        self.give_control_away(now, crossing, out);
    }

    /// Entrega o controle ao par.
    fn give_control_away(&mut self, now: Timestamp, crossing: Crossing, out: &mut CommandBatch) {
        if !self.move_to(Phase::Engaged, out) {
            return;
        }

        let position = self.entry_on_peer(crossing);
        let message = Control::EnterScreen {
            entering_edge: crossing.exit_edge.opposite(),
            position,
            // O estado completo vai junto: o cliente começa sincronizado, sem depender do
            // snapshot seguinte (`docs/03-protocolo.md` §6).
            state: self.input_state.clone(),
        };
        self.send(now, Message::Control(message), out);

        out.push(Command::SuppressLocalInput(true));
        out.push(Command::Notify(Notice::ControlMoved { remote: true }));

        self.pending_pointer = PointerDelta::ZERO;
        self.clock.last_pointer = now;
        self.clock.last_snapshot = now;
    }

    /// Onde o ponteiro deve aparecer no par.
    ///
    /// Com o arranjo do par conhecido, calcula direto. Sem ele — o `Screens` ainda não chegou
    /// — usa a borda do monitor zero na mesma fração. É melhor entrar na tela errada de um
    /// arranjo múltiplo do que não entrar.
    fn entry_on_peer(&self, crossing: Crossing) -> PointerPosition {
        if let Some(peer) = self.peer_screens.as_ref() {
            return ir_geometry::entry_position(peer, crossing);
        }
        let (x, y) = match crossing.exit_edge.opposite() {
            Edge::Left => (0, crossing.fraction),
            Edge::Right => (u16::MAX, crossing.fraction),
            Edge::Top => (crossing.fraction, 0),
            Edge::Bottom => (crossing.fraction, u16::MAX),
        };
        PointerPosition {
            monitor: MonitorId(0),
            x,
            y,
        }
    }

    pub(super) fn flush_pointer_if_due(&mut self, now: Timestamp, out: &mut CommandBatch) {
        if self.pending_pointer.is_zero() {
            return;
        }
        if !now.elapsed_at_least(
            self.clock.last_pointer,
            self.config.timings.pointer_interval,
        ) {
            return;
        }
        let delta = core::mem::replace(&mut self.pending_pointer, PointerDelta::ZERO);
        self.clock.last_pointer = now;
        let mods = self.input_state.modifiers;
        self.send(
            now,
            Message::Pointer(PointerMessage::Motion { delta, mods }),
            out,
        );
    }

    pub(super) fn send_snapshot_if_due(&mut self, now: Timestamp, out: &mut CommandBatch) {
        if !now.elapsed_at_least(
            self.clock.last_snapshot,
            self.config.timings.snapshot_interval,
        ) {
            return;
        }
        self.clock.last_snapshot = now;
        let message = Control::StateSnapshot {
            state: self.input_state.clone(),
            position: self.local_position(),
        };
        self.send(now, Message::Control(message), out);
    }

    pub(super) fn local_position(&self) -> PointerPosition {
        self.local_screens.as_ref().map_or(
            PointerPosition {
                monitor: MonitorId(0),
                x: 0,
                y: 0,
            },
            |desktop| desktop.to_position(self.pointer),
        )
    }

    pub(super) fn on_local_key(
        &mut self,
        now: Timestamp,
        usage: HidUsage,
        pressed: bool,
        out: &mut CommandBatch,
    ) {
        if self.config.role.captures() {
            self.held_here = self.held_here.applying(usage, pressed);
            if self.phase.is_established() && self.take_shortcut(now, usage, pressed, out) {
                return;
            }
        }
        if !self.is_forwarding() {
            return;
        }
        self.input_state.apply_key(usage, pressed);
        let mods = self.input_state.modifiers;
        let message = if pressed {
            InputMessage::KeyDown { usage, mods }
        } else {
            InputMessage::KeyUp { usage, mods }
        };
        self.send(now, Message::Input(message), out);
    }

    pub(super) fn on_local_button(
        &mut self,
        now: Timestamp,
        button: Button,
        pressed: bool,
        out: &mut CommandBatch,
    ) {
        if !self.is_forwarding() {
            return;
        }
        self.input_state.apply_button(button, pressed);
        let mods = self.input_state.modifiers;
        let message = if pressed {
            InputMessage::ButtonDown { button, mods }
        } else {
            InputMessage::ButtonUp { button, mods }
        };
        self.send(now, Message::Input(message), out);
    }

    pub(super) fn on_local_wheel(
        &mut self,
        now: Timestamp,
        delta: WheelDelta,
        out: &mut CommandBatch,
    ) {
        if !self.is_forwarding() {
            return;
        }
        let mods = self.input_state.modifiers;
        self.send(
            now,
            Message::Input(InputMessage::Wheel { delta, mods }),
            out,
        );
    }

    /// Se eventos locais devem ser encaminhados ao par neste instante.
    const fn is_forwarding(&self) -> bool {
        self.config.role.captures() && matches!(self.phase, Phase::Engaged)
    }

    /// O par avisou que o ponteiro voltou pela borda dele.
    pub(super) fn on_edge_reached(
        &mut self,
        now: Timestamp,
        edge: Edge,
        position: PointerPosition,
        out: &mut CommandBatch,
    ) {
        if self.config.role != Role::Server || self.phase != Phase::Engaged {
            return;
        }

        let fraction = self.fraction_on_peer(edge, position);
        self.send(
            now,
            Message::Control(Control::LeaveScreen {
                leaving_edge: edge,
                position,
            }),
            out,
        );
        self.take_control_back(fraction, out);
    }

    /// Onde, ao longo da borda do par, o ponteiro estava.
    ///
    /// Sem o arranjo do par conhecido, assume o meio da borda: entrar no meio é sempre
    /// utilizável, enquanto entrar num canto pode disparar outra travessia.
    fn fraction_on_peer(&self, edge: Edge, position: PointerPosition) -> u16 {
        self.peer_screens.as_ref().map_or(u16::MAX / 2, |peer| {
            let point = peer.from_position(position);
            peer.bounds().fraction_along(edge, point)
        })
    }

    /// Traz o controle de volta para esta máquina, pondo o ponteiro na borda certa.
    pub(super) fn take_control_back(&mut self, fraction: u16, out: &mut CommandBatch) {
        self.hand_control_back(out);

        if let Some(desktop) = self.local_screens.as_ref() {
            // Entrar pela mesma borda por onde saiu: é o que faz a volta parecer contínua.
            let crossing = Crossing {
                exit_edge: self.config.peer_edge.opposite(),
                fraction,
            };
            let position = ir_geometry::entry_position(desktop, crossing);
            self.pointer = desktop.from_position(position);
            out.push(Command::WarpPointer(position));
        }
    }

    /// Devolve o controle para esta máquina, soltando tudo e liberando a entrada local.
    ///
    /// Usado no retorno normal, na emergência, na troca de portador e na queda. É o mesmo
    /// caminho nos quatro casos de propósito: um caminho de liberação por situação é como se
    /// esquece um deles.
    pub(super) fn hand_control_back(&mut self, out: &mut CommandBatch) {
        self.release_everything(out);
        self.pending_pointer = PointerDelta::ZERO;
        if self.config.role.captures() {
            out.push(Command::SuppressLocalInput(false));
        }
        if self.phase == Phase::Engaged {
            self.phase = Phase::Ready;
            out.push(Command::Notify(Notice::ControlMoved { remote: false }));
        }
    }
}
