//! O par usando esta tela: recebe, injeta e detecta a volta ([`Phase::Receiving`]).
//!
//! Qualquer um dos dois computadores faz isto (ADR-0014). Duas responsabilidades que só existem
//! aqui:
//!
//! - **converter delta em posição absoluta.** O par manda deltas crus; quem conhece o
//!   arranjo de telas desta máquina é ela mesma. Injetar movimento relativo faria o sistema
//!   aplicar aceleração a deltas que já vêm acelerados (`docs/05-windows.md` §4.2).
//! - **detectar a travessia de volta.** É aqui que o ponteiro está, então é aqui que se
//!   percebe que ele encostou na borda que devolve o controle.

use ir_geometry::{Movement, advance};
use ir_proto::input::{InputState, PointerPosition};
use ir_proto::message::{Control, Feedback, InputMessage, Message, PointerMessage};

use crate::event::{Command, CommandBatch, Injection, Notice};
use crate::phase::Phase;
use crate::session::Session;
use crate::time::Timestamp;

impl Session {
    /// O par entregou o controle a esta tela.
    pub(super) fn on_enter_screen(
        &mut self,
        now: Timestamp,
        position: PointerPosition,
        state: &InputState,
        out: &mut CommandBatch,
    ) {
        if !self.phase.is_established() {
            return;
        }
        if !self.config.policy.receives() || self.refusals.here {
            // Esta máquina não é controlada, ou está numa tela protegida que recusa o par. Ele não
            // devia ter atravessado — sabe pelo `Hello` e pelo `ProtectedDesktop` —, mas se
            // atravessou, a resposta é devolver, e não deixá-lo mandando ao nada.
            self.send(now, Message::Control(Control::Reclaim), out);
            return;
        }
        if self.phase == Phase::Sending && !self.yield_crossing(out) {
            return; // os dois atravessaram juntos, e quem cede é o par
        }
        if !self.move_to(Phase::Receiving, out) {
            return;
        }
        self.start_receiving(now);

        if let Some(desktop) = self.local_screens.as_ref() {
            self.pointer = desktop.from_position(position);
        }
        out.push(Command::Inject(Injection::Pointer(position)));
        out.push(Command::Notify(Notice::ControlMoved { remote: false }));

        // O estado veio junto com a entrega, então já se começa sincronizado.
        self.reconcile(state, out);
    }

    /// O par levou o controle de volta pela borda dele.
    pub(super) fn on_leave_screen(&mut self, out: &mut CommandBatch) {
        if self.phase != Phase::Receiving {
            return;
        }
        self.release_everything(out);
        self.phase = Phase::Ready;
        out.push(Command::Notify(Notice::ControlMoved { remote: true }));
    }

    pub(super) fn on_input_message(&mut self, message: InputMessage, out: &mut CommandBatch) {
        if !self.is_injecting() {
            return;
        }

        // O estado de modificadores viaja em toda mensagem; divergência é corrigida antes de
        // aplicar o evento. É o que elimina a categoria do modificador preso
        // (`docs/03-protocolo.md` §5).
        if let Some(declared) = message.declared_modifiers() {
            self.align_modifiers(declared, out);
        }

        match message {
            InputMessage::KeyDown { usage, .. } => {
                self.input_state.apply_key(usage, true);
                out.push(Command::Inject(Injection::Key {
                    usage,
                    pressed: true,
                }));
            }
            InputMessage::KeyUp { usage, .. } => {
                self.input_state.apply_key(usage, false);
                out.push(Command::Inject(Injection::Key {
                    usage,
                    pressed: false,
                }));
            }
            InputMessage::ButtonDown { button, .. } => {
                self.input_state.apply_button(button, true);
                out.push(Command::Inject(Injection::Button {
                    button,
                    pressed: true,
                }));
            }
            InputMessage::ButtonUp { button, .. } => {
                self.input_state.apply_button(button, false);
                out.push(Command::Inject(Injection::Button {
                    button,
                    pressed: false,
                }));
            }
            InputMessage::Wheel { delta, .. } => {
                out.push(Command::Inject(Injection::Wheel(delta)));
            }
            InputMessage::ReleaseAll => self.release_everything(out),
            _ => {}
        }
    }

    pub(super) fn on_pointer_message(
        &mut self,
        now: Timestamp,
        message: PointerMessage,
        out: &mut CommandBatch,
    ) {
        if !self.is_injecting() {
            return;
        }
        self.align_modifiers(message.declared_modifiers(), out);

        match message {
            PointerMessage::Position { position, .. } => {
                if let Some(desktop) = self.local_screens.as_ref() {
                    self.pointer = desktop.from_position(position);
                }
                out.push(Command::Inject(Injection::Pointer(position)));
            }
            PointerMessage::Motion { delta, .. } => {
                let Some(desktop) = self.local_screens.as_ref() else {
                    return; // sem arranjo não há como converter delta em posição
                };
                match advance(desktop, self.pointer, delta, self.config.peer_edge) {
                    Movement::Stayed(point) => {
                        self.pointer = point;
                        let position = desktop.to_position(point);
                        out.push(Command::Inject(Injection::Pointer(position)));
                    }
                    Movement::Crossed(_) => self.report_edge_return(now, out),
                }
            }
            _ => {}
        }
    }

    /// O par mandou o estado completo. Reconcilia.
    pub(super) fn on_snapshot(
        &mut self,
        state: &InputState,
        position: PointerPosition,
        out: &mut CommandBatch,
    ) {
        if !self.is_injecting() {
            return;
        }
        let _ = position; // a posição do servidor é informativa; a nossa é a que vale
        self.reconcile(state, out);
    }

    /// Faz o estado local virar o estado desejado, soltando e pressionando o que faltar.
    ///
    /// **Idempotente**: reconciliar duas vezes com o mesmo alvo não produz ação nenhuma na
    /// segunda. É essa propriedade que permite mandar snapshot a cada 250 ms sem risco de
    /// oscilação, e é ela que sustenta a meta de zero teclas presas
    /// (`docs/01-visao-e-escopo.md` §6).
    fn reconcile(&mut self, desired: &InputState, out: &mut CommandBatch) {
        let mut released = 0u8;
        let mut pressed = 0u8;

        let to_release: Vec<_> = self.input_state.keys.difference(&desired.keys).collect();
        for usage in to_release {
            self.input_state.apply_key(usage, false);
            out.push(Command::Inject(Injection::Key {
                usage,
                pressed: false,
            }));
            released = released.saturating_add(1);
        }

        let to_press: Vec<_> = desired.keys.difference(&self.input_state.keys).collect();
        for usage in to_press {
            self.input_state.apply_key(usage, true);
            out.push(Command::Inject(Injection::Key {
                usage,
                pressed: true,
            }));
            pressed = pressed.saturating_add(1);
        }

        let stuck: Vec<_> = self
            .input_state
            .buttons
            .missing_from(desired.buttons)
            .iter()
            .collect();
        for button in stuck {
            self.input_state.apply_button(button, false);
            out.push(Command::Inject(Injection::Button {
                button,
                pressed: false,
            }));
            released = released.saturating_add(1);
        }

        let missing: Vec<_> = desired
            .buttons
            .missing_from(self.input_state.buttons)
            .iter()
            .collect();
        for button in missing {
            self.input_state.apply_button(button, true);
            out.push(Command::Inject(Injection::Button {
                button,
                pressed: true,
            }));
            pressed = pressed.saturating_add(1);
        }

        if released > 0 || pressed > 0 {
            out.push(Command::Notify(Notice::Reconciled { released, pressed }));
        }
    }

    /// Corrige modificadores divergentes antes de aplicar um evento.
    fn align_modifiers(&mut self, declared: ir_proto::input::Modifiers, out: &mut CommandBatch) {
        let applied = self.input_state.modifiers;
        if applied == declared {
            return;
        }
        let mut desired = self.input_state.clone();
        desired.modifiers = declared;
        for usage in MODIFIER_USAGES {
            let bit = ir_proto::input::Modifiers::from_usage(usage);
            let Some(bit) = bit else { continue };
            let should = declared.contains(bit);
            if should != applied.contains(bit) {
                self.input_state.apply_key(usage, should);
                out.push(Command::Inject(Injection::Key {
                    usage,
                    pressed: should,
                }));
            }
        }
    }

    /// Avisa o par de que o ponteiro voltou pela borda, e para de injetar.
    pub(super) fn report_edge_return(&mut self, now: Timestamp, out: &mut CommandBatch) {
        if self.phase != Phase::Receiving {
            return;
        }
        let position = self.local_screens.as_ref().map_or(
            PointerPosition {
                monitor: ir_proto::ids::MonitorId(0),
                x: 0,
                y: 0,
            },
            |d| d.to_position(self.pointer),
        );

        // Solta tudo **antes** de anunciar: se a mensagem se perder, o servidor cai por
        // tempo e o estado local já está limpo de qualquer jeito.
        self.release_everything(out);
        self.phase = Phase::Ready;

        self.send(
            now,
            Message::Feedback(Feedback::EdgeReached {
                edge: self.config.peer_edge,
                position,
            }),
            out,
        );
        out.push(Command::Notify(Notice::ControlMoved { remote: true }));
    }

    const fn is_injecting(&self) -> bool {
        matches!(self.phase, Phase::Receiving)
    }
}

/// Os oito modificadores, na ordem do relatório HID.
const MODIFIER_USAGES: [ir_proto::input::HidUsage; 8] = [
    ir_proto::input::HidUsage::LEFT_CTRL,
    ir_proto::input::HidUsage::LEFT_SHIFT,
    ir_proto::input::HidUsage::LEFT_ALT,
    ir_proto::input::HidUsage::LEFT_GUI,
    ir_proto::input::HidUsage::RIGHT_CTRL,
    ir_proto::input::HidUsage::RIGHT_SHIFT,
    ir_proto::input::HidUsage::RIGHT_ALT,
    ir_proto::input::HidUsage::RIGHT_GUI,
];
