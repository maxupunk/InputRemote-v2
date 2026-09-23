//! Despacho dos quadros que chegam, e a emergência.

use ir_geometry::Desktop;
use ir_proto::carrier::Carrier;
use ir_proto::channel::ChannelId;
use ir_proto::frame::{Frame, Sequence};
use ir_proto::message::{Control, Feedback, Message};

use crate::config::Role;
use crate::event::{Command, CommandBatch, Notice};
use crate::phase::Phase;
use crate::reliability::{Delivery, ReliableChannels};
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
        self.clock.mark_carrier_rx(carrier, now);
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

        // O canal do ponteiro não tem confirmação, mas tem ordem: o mais recente vence, e o que
        // chega igual ou atrasado é descartado (`docs/03-protocolo.md` §4.2). Na rota dupla toda
        // amostra chega duas vezes; aplicar as duas moveria o cursor o dobro.
        let channel = frame.channel();
        if !ReliableChannels::covers(channel) {
            if channel == ChannelId::Pointer && !self.admit_pointer(frame.seq) {
                return;
            }
            self.wins.count(carrier);
            self.dispatch_message(now, frame.message, out);
            return;
        }

        // Todo canal confiável passa pela ordenação e pela detecção de repetição, qualquer que
        // seja o portador: a sessão trata toda rota como datagrama (`route`).
        match self.reliability.accept(channel, frame) {
            // Repetição: descartar sem processar não é otimização. Reaplicar um `KeyDown`
            // gravado do ar seria redigitar o que o usuário digitou
            // (`docs/04-seguranca.md` §2).
            // Adiantado espera o buraco à frente ser preenchido: entregar agora poria um
            // `KeyDown` retransmitido **depois** do `KeyUp` que o soltaria, e a tecla ficaria
            // presa para sempre.
            Delivery::Duplicate | Delivery::Buffered => {}
            Delivery::Ready(frames) => {
                self.wins.count(carrier);
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
            Message::Clipboard(message) => self.on_clipboard_message(now, message, out),
            // Dados vão pelo canal próprio, fora da sessão; mensagens de versões futuras não
            // são tratadas. Ignorar é o correto, e o par não é penalizado por oferecê-las.
            _ => {}
        }
    }

    /// Se esta amostra de ponteiro é mais nova que a última aplicada — e, sendo, passa a ser ela.
    fn admit_pointer(&mut self, seq: Sequence) -> bool {
        if self
            .last_pointer_rx
            .is_some_and(|last| !seq.is_newer_than(last))
        {
            return false;
        }
        self.last_pointer_rx = Some(seq);
        true
    }

    /// Adota o portador por onde um quadro chegou, quando ainda não há nenhum.
    ///
    /// Sem isto, quem recebe o `Hello` primeiro não consegue responder — `send` não tem por
    /// onde mandar — e acaba iniciando o próprio handshake, zerando as janelas e fazendo o
    /// `Hello` retransmitido do outro lado parecer novo. A regra passa a ser simples: **quem
    /// ouve primeiro, responde**. Receber um quadro por um portador é prova de que ele
    /// funciona; esperar o aviso local de que ele subiu é esperar informação que já chegou.
    ///
    /// Com a sessão de pé, um quadro por um portador de fora da rota é o par que já juntou esse
    /// portador à rota dele: se ele também está de pé daqui, entra na rota deste lado também.
    fn adopt_carrier_if_needed(&mut self, carrier: Carrier, out: &mut CommandBatch) {
        if !carrier.carries_input() {
            return;
        }
        if self.route.is_some() {
            self.widen_route(carrier, out);
            return;
        }
        self.available.set(carrier, true);
        self.route = Some(super::Route::Single(carrier));
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
            Control::Reach { radio } => Self::on_reach(radio, out),
            Control::NetworkPower(state) => Self::on_peer_network_power(state, out),
            Control::DisableNetworkPowerSaving => Self::on_network_power_fix_requested(out),
            // Só o controlado gera Ctrl+Alt+Del; pedido ao contrário é engano do par, e ignorado.
            // Quem gera o Ctrl+Alt+Del, e decide se pode, é a periferia do controlado.
            Control::SecureAttention if self.config.role == crate::config::Role::Client => {
                out.push(Command::SecureAttention);
            }
            Control::ProtectedDesktop { refused } => {
                out.push(Command::Notify(Notice::PeerProtectedDesktop { refused }));
            }
            // Só quem é controlado bloqueia a pedido; o contrário seria o par trancando quem digita.
            Control::LockScreen if self.config.role == crate::config::Role::Client => {
                out.push(Command::LockScreen);
            }
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
