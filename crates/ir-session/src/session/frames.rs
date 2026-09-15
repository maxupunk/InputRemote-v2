//! Despacho dos quadros que chegam, e a emergência.

use ir_geometry::Desktop;
use ir_proto::carrier::Carrier;
use ir_proto::frame::Frame;
use ir_proto::message::{Control, Feedback, Message};

use crate::config::Role;
use crate::event::{Command, CommandBatch, Notice};
use crate::phase::Phase;
use crate::reliability::Delivery;
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
        // Antes de tudo: de qual sessão é este quadro. Um quadro de uma encarnação que já acabou
        // não prova nada — nem que o par está vivo, nem que o portador serve —, e deixá-lo passar
        // foi o que ancorou sessões novas em numeração velha (`incarnation`).
        if !self.admit(now, carrier, &frame, out) {
            return;
        }

        // Quadro chegando é prova de que o par está vivo, qualquer que seja o conteúdo.
        // Atualizar aqui, antes de qualquer despacho, é o que impede a sessão de cair por
        // tempo enquanto processa uma rajada.
        self.clock.last_rx = now;
        self.arm_link_timeout(now, out);
        self.adopt_carrier_if_needed(carrier, out);

        // A confirmação vem antes de qualquer despacho: ela libera janela do nosso lado, e
        // fazê-lo primeiro impede que uma rajada encha a janela enquanto é processada.
        if let Some(carried) = frame.ack {
            self.reliability.on_ack(carried.channel, now, carried.ack);
        }

        // Uma confirmação pura não entra na ordenação nem na detecção de repetição: ela não
        // faz parte do fluxo, e o que ela carregava já foi aplicado acima.
        if super::is_bare_ack(&frame) {
            return;
        }

        // O adeus também viaja fora da ordem: esperar a vez dele seria esperar por uma sessão
        // que já acabou, e o par do outro lado ficaria segurando as teclas até o prazo vencer.
        if super::is_farewell(&frame) {
            self.dispatch_message(now, frame.message, out);
            return;
        }

        let channel = frame.channel();
        if !channel.needs_app_reliability(carrier) {
            self.dispatch_message(now, frame.message, out);
            return;
        }

        match self.reliability.accept(channel, frame) {
            // Repetição: descartar sem processar não é otimização. Reaplicar um `KeyDown`
            // gravado do ar seria redigitar o que o usuário digitou
            // (`docs/04-seguranca.md` §2).
            // Adiantado espera o buraco à frente ser preenchido: entregar agora poria um
            // `KeyDown` retransmitido **depois** do `KeyUp` que o soltaria, e a tecla ficaria
            // presa para sempre.
            Delivery::Duplicate | Delivery::Buffered => {}
            Delivery::Ready(frames) => {
                for frame in frames {
                    self.dispatch_message(now, frame.message, out);
                }
            }
            Delivery::Overflow => {
                // Perdeu-se mais do que a fila consegue reparar. Entregar com lacuna deixaria
                // tecla presa; cair é a escolha menos ruim.
                self.tear_down(now, crate::event::LinkDown::Timeout, out);
            }
        }
    }

    /// Entrega uma mensagem ao tratador do seu canal.
    fn dispatch_message(&mut self, now: Timestamp, message: Message, out: &mut CommandBatch) {
        match message {
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

    /// Adota o portador por onde um quadro chegou, quando ainda não há nenhum.
    ///
    /// Sem isto, quem recebe o `Hello` primeiro não consegue responder — `send` não tem por
    /// onde mandar — e acaba iniciando o próprio handshake, zerando as janelas e fazendo o
    /// `Hello` retransmitido do outro lado parecer novo. A regra passa a ser simples: **quem
    /// ouve primeiro, responde**. Receber um quadro por um portador é prova de que ele
    /// funciona; esperar o aviso local de que ele subiu é esperar informação que já chegou.
    fn adopt_carrier_if_needed(&mut self, carrier: Carrier, out: &mut CommandBatch) {
        if self.carrier.is_some() || !carrier.carries_input() {
            return;
        }
        self.available.set(carrier, true);
        self.carrier = Some(carrier);
        if self.phase == Phase::Offline {
            self.move_to(Phase::Handshaking, out);
        }
    }

    fn on_control(&mut self, now: Timestamp, control: Control, out: &mut CommandBatch) {
        match control {
            Control::Hello(greeting) => self.on_greeting(now, greeting, false, out),
            Control::HelloAck(greeting) => self.on_greeting(now, greeting, true, out),
            Control::Screens(layout) => {
                self.peer_screens = Desktop::from_layout(&layout);
            }
            Control::EdgeConfig { peer_edge } => self.on_edge_config(peer_edge, out),
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
            // quem a consome é a camada de confiabilidade sobre UDP.
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
