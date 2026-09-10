//! Despacho dos quadros que chegam, e a emergência.

use ir_geometry::Desktop;
use ir_proto::carrier::Carrier;
use ir_proto::frame::Frame;
use ir_proto::message::{Control, Feedback, Message};

use crate::config::Role;
use crate::event::{Command, CommandBatch, Notice};
use crate::phase::Phase;
use crate::session::Session;
use crate::time::Timestamp;

impl Session {
    pub(super) fn on_frame(
        &mut self,
        now: Timestamp,
        carrier: Carrier,
        frame: Frame,
        out: &mut CommandBatch,
    ) {
        // Quadro chegando é prova de que o par está vivo, qualquer que seja o conteúdo.
        // Atualizar aqui, antes de qualquer despacho, é o que impede a sessão de cair por
        // tempo enquanto processa uma rajada.
        self.clock.last_rx = now;
        self.arm_link_timeout(now, out);

        if self.carrier != Some(carrier) && frame.channel().allows(carrier) {
            // Chegou por um portador que não é o ativo. Acontece na troca, com mensagens em
            // trânsito. Aceita-se o conteúdo — a alternativa seria perder um `KeyUp`.
        }

        match frame.message {
            Message::Control(control) => self.on_control(now, control, out),
            Message::Input(message) => self.on_input_message(message, out),
            Message::Pointer(message) => self.on_pointer_message(now, message, out),
            Message::Feedback(feedback) => self.on_feedback(now, feedback, out),
            // Clipboard, dados e mensagens de versões futuras ainda não são tratados pelo
            // núcleo. Ignorar é o correto até que sejam, e o par não é penalizado por
            // oferecê-los.
            _ => {}
        }
    }

    fn on_control(&mut self, now: Timestamp, control: Control, out: &mut CommandBatch) {
        match control {
            Control::Hello(greeting) => self.on_greeting(now, greeting, false, out),
            Control::HelloAck(greeting) => self.on_greeting(now, greeting, true, out),
            Control::Screens(layout) => {
                self.peer_screens = Desktop::from_layout(&layout);
            }
            Control::EnterScreen {
                entering_edge,
                position,
                state,
            } => {
                self.on_enter_screen(entering_edge, position, &state, out);
            }
            Control::LeaveScreen { .. } => self.on_leave_screen(out),
            Control::StateSnapshot { state, position } => {
                self.on_snapshot(&state, position, out);
            }
            Control::Ping { stamp_micros } => {
                self.send(now, Message::Control(Control::Pong { stamp_micros }), out);
            }
            Control::Pong { stamp_micros } => self.on_pong(now, stamp_micros, out),
            Control::Bye { reason } => self.on_bye(now, reason, out),
            Control::Error { code, fatal } => {
                out.push(Command::Notify(Notice::ProtocolError { code, fatal }));
                if fatal || code.is_always_fatal() {
                    self.tear_down(now, crate::event::LinkDown::TransportFailed, out);
                }
            }
            // `AckOnly` não tem conteúdo: a confirmação viaja no campo `ack` do quadro, e
            // quem a consome é a camada de confiabilidade sobre UDP. `EdgeConfig` é
            // informativo — a borda vem das preferências locais, não do par.
            _ => {}
        }
    }

    fn on_feedback(&mut self, now: Timestamp, feedback: Feedback, out: &mut CommandBatch) {
        match feedback {
            Feedback::EdgeReached { edge, position } => {
                self.on_edge_reached(now, edge, position, out);
            }
            Feedback::EmergencyRelease => {
                // O usuário pediu socorro do outro lado. Retomar o controle é o certo: ele
                // está com um teclado que não responde onde espera.
                if self.config.role == Role::Server && self.phase == Phase::Engaged {
                    self.hand_control_back(out);
                }
            }
            Feedback::StateReconciled { released, pressed } => {
                out.push(Command::Notify(Notice::Reconciled { released, pressed }));
            }
            _ => {}
        }
    }

    /// O atalho de emergência local.
    ///
    /// Devolve o controle e solta tudo **imediatamente**, mesmo com o enlace saudável. É a
    /// saída de que o usuário precisa quando alguma coisa deu errado e ele não sabe o quê, e
    /// por isso não depende de resposta do par: age local primeiro, avisa depois.
    pub(super) fn on_emergency(&mut self, now: Timestamp, out: &mut CommandBatch) {
        if self.phase != Phase::Engaged {
            // Mesmo sem sessão em uso, soltar é barato e é o que o usuário pediu.
            self.release_everything(out);
            return;
        }

        match self.config.role {
            Role::Server => {
                self.send(
                    now,
                    Message::Input(ir_proto::message::InputMessage::ReleaseAll),
                    out,
                );
                self.hand_control_back(out);
            }
            Role::Client => {
                self.send(now, Message::Feedback(Feedback::EmergencyRelease), out);
                self.report_edge_return(now, out);
            }
        }
    }
}
