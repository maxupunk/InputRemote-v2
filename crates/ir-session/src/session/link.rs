//! Enlace: subida, handshake, prazos e queda.
//!
//! A regra que governa este módulo: **em toda queda, solta-se tudo antes de qualquer outra
//! coisa.** Um enlace que morre com `Ctrl` pressionado do outro lado deixa a máquina do
//! usuário inutilizável até o próximo reinício, e é a falha mais cara que este produto pode
//! cometer (`docs/02-arquitetura.md` §8).

use ir_proto::carrier::Carrier;
use ir_proto::frame::{Frame, Sequence};
use ir_proto::message::{Control, DisconnectReason, ErrorCode, Greeting, Message};
use ir_proto::version;

use crate::config::Role;
use crate::event::{CarrierChoice, Command, CommandBatch, LinkDown, Notice, TimerId};
use crate::phase::Phase;
use crate::reliability::Due;
use crate::session::Session;
use crate::session::state::{Clock, PeerInfo};
use crate::time::{Millis, Timestamp};

/// Intervalo entre confirmações puras.
///
/// Origem: `docs/03-protocolo.md` §4.1 — a cada 20 ms enquanto houver algo pendente.
const ACK_INTERVAL: Millis = Millis(20);

impl Session {
    pub(super) fn on_carrier_up(
        &mut self,
        now: Timestamp,
        carrier: Carrier,
        out: &mut CommandBatch,
    ) {
        self.available.set(carrier, true);

        // TCP é o canal de dados; ele não estabelece sessão nem troca a de entrada.
        if !carrier.carries_input() {
            return;
        }

        let Some((chosen, why)) = self.available.pick_input_carrier(self.pinned) else {
            return;
        };

        // Já estamos usando o melhor portador disponível: nada a fazer.
        if self.carrier == Some(chosen) && self.phase.is_established() {
            return;
        }

        // Trocar de portador com o controle no par exige soltar tudo antes: as mensagens em
        // trânsito no portador antigo não chegam, e entre elas pode estar um `KeyUp`.
        if self.phase.may_hold_input() {
            self.hand_control_back(out);
        }

        self.begin_handshake(now, chosen, why, out);
    }

    pub(super) fn on_carrier_down(
        &mut self,
        now: Timestamp,
        carrier: Carrier,
        reason: LinkDown,
        out: &mut CommandBatch,
    ) {
        self.available.set(carrier, false);

        if self.carrier != Some(carrier) {
            return; // caiu um portador que não estava em uso
        }

        self.tear_down(now, reason, out);

        // A política única decide o substituto, e o motivo aparece na interface.
        if let Some((next, why)) = self.available.pick_input_carrier(self.pinned) {
            self.begin_handshake(now, next, why, out);
        }
    }

    /// Começa uma sessão pelo portador dado.
    fn begin_handshake(
        &mut self,
        now: Timestamp,
        carrier: Carrier,
        why: CarrierChoice,
        out: &mut CommandBatch,
    ) {
        self.carrier = Some(carrier);
        self.peer = None;
        // Sequências zeradas: uma herdada da sessão anterior faria o par descartar as
        // primeiras mensagens da nova.
        self.seqs.reset();
        self.reliability.reset();
        // Encarnação nova: o par descarta o que ainda estiver voando da anterior, e daqui em
        // diante só um aperto de mão dele conta.
        self.incarnations.start_local();
        self.incarnations.forget_peer();
        self.clock = Clock::started_at(now);

        if !self.move_to(Phase::Handshaking, out) {
            return;
        }
        out.push(Command::Notify(Notice::CarrierChanged { carrier, why }));

        let greeting = self.greeting();
        self.send(now, Message::Control(Control::Hello(greeting)), out);
        self.arm_link_timeout(now, out);
    }

    fn greeting(&self) -> Greeting {
        Greeting {
            version: version::CURRENT,
            machine: self.identity.machine,
            name: self.identity.name.clone(),
            capabilities: self.identity.capabilities,
        }
    }

    /// Trata `Hello` e `HelloAck`.
    pub(super) fn on_greeting(
        &mut self,
        now: Timestamp,
        greeting: Greeting,
        is_reply: bool,
        out: &mut CommandBatch,
    ) {
        let Ok(negotiated) = version::negotiate(greeting.version) else {
            // Sem denominador comum não há sessão. O par recebe o código; o detalhe fica no
            // log local (`docs/04-seguranca.md` §7).
            self.send(
                now,
                Message::Control(Control::Error {
                    code: ErrorCode::UnsupportedMessage,
                    fatal: true,
                }),
                out,
            );
            self.tear_down(now, LinkDown::TransportFailed, out);
            return;
        };

        self.peer = Some(PeerInfo {
            machine: greeting.machine,
            name: greeting.name,
            capabilities: greeting.capabilities,
            version: negotiated.version,
        });

        if !is_reply {
            let reply = self.greeting();
            self.send(now, Message::Control(Control::HelloAck(reply)), out);
        }

        self.establish(now, out);
    }

    fn establish(&mut self, now: Timestamp, out: &mut CommandBatch) {
        // Um `Hello` atrasado ou retransmitido chega numa sessão que já está de pé. Deixar
        // `establish` correr de novo rebaixaria a fase de `Engaged` para `Ready` — devolvendo
        // o controle no meio do uso — e reanunciaria a conexão à interface. A identidade do
        // par já foi atualizada por quem chamou; aqui não há mais nada a fazer.
        if self.phase.is_established() {
            return;
        }
        if !self.move_to(Phase::Ready, out) {
            return;
        }
        self.clock = Clock::started_at(now);

        if let (Some(peer), Some(carrier)) = (self.peer.as_ref(), self.carrier) {
            out.push(Command::Notify(Notice::Connected {
                peer: peer.name.clone(),
                carrier,
            }));
        }

        // O par precisa do nosso arranjo para saber onde o ponteiro entra.
        if let Some(desktop) = self.local_screens.as_ref() {
            let layout = desktop.to_layout();
            self.send(now, Message::Control(Control::Screens(layout)), out);
        }

        self.arm_heartbeat(now, out);
        self.arm_link_timeout(now, out);
    }

    /// Encerra a sessão corrente.
    pub(super) fn tear_down(&mut self, now: Timestamp, reason: LinkDown, out: &mut CommandBatch) {
        // Avisa o par antes de morrer, quando a decisão é nossa e ainda há por onde falar.
        //
        // Sem isto, quem fica do outro lado só percebe pelo próprio prazo de queda — até um
        // segundo depois — e segura as teclas até lá. O adeus quase sempre chega, porque a
        // maioria das quedas é por perda parcial, não por meio morto. E ele **não** entra na
        // janela de retransmissão: não haveria quem confirmasse, e mandar por `send` poderia
        // reentrar aqui pela janela cheia.
        if self.phase.is_established()
            && !matches!(reason, LinkDown::PeerClosed(_) | LinkDown::PeerRestarted)
            && let Some(carrier) = self.carrier
        {
            let farewell = Frame::new(
                Message::Control(Control::Bye {
                    reason: reason.as_disconnect_reason(),
                }),
                Sequence::ZERO,
            )
            .in_epoch(self.incarnations.local());
            out.push(Command::Send {
                carrier,
                frame: farewell,
            });
        }

        // Primeiro soltar, depois qualquer outra coisa. A ordem é contrato.
        if self.phase.may_hold_input() {
            self.release_everything(out);
            if self.config.role.captures() {
                out.push(Command::SuppressLocalInput(false));
            }
        }

        let will_retry = reason.should_retry();
        out.push(Command::Notify(Notice::Disconnected { reason, will_retry }));

        self.phase = Phase::Offline;
        self.carrier = None;
        self.peer = None;
        self.peer_screens = None;
        self.pending_pointer = ir_proto::input::PointerDelta::ZERO;
        self.seqs.reset();
        self.reliability.reset();
        self.incarnations.forget_peer();

        for timer in [TimerId::Heartbeat, TimerId::Snapshot, TimerId::PointerFlush] {
            out.push(Command::ClearTimer(timer));
        }
        out.push(Command::ClearTimer(TimerId::LinkTimeout));

        if will_retry {
            out.push(Command::SetTimer {
                id: TimerId::Reconnect,
                at: now.plus(self.config.timings.reconnect_delay),
            });
        }
    }

    /// Encerra por vontade própria.
    ///
    /// O aviso ao par é responsabilidade de [`Session::tear_down`], que o manda em toda queda
    /// decidida por este lado — não só nesta.
    pub fn stop(&mut self, now: Timestamp, reason: LinkDown, out: &mut CommandBatch) {
        self.tear_down(now, reason, out);
    }

    pub(super) fn on_bye(
        &mut self,
        now: Timestamp,
        reason: DisconnectReason,
        out: &mut CommandBatch,
    ) {
        self.tear_down(now, Self::peer_closed(reason), out);
    }

    pub(super) fn on_tick(&mut self, now: Timestamp, out: &mut CommandBatch) {
        if self.phase == Phase::Offline {
            return;
        }

        if now.elapsed_at_least(self.clock.last_rx, self.config.timings.link_timeout) {
            self.tear_down(now, LinkDown::Timeout, out);
            return;
        }

        if now.elapsed_at_least(self.clock.last_heartbeat, self.config.timings.heartbeat) {
            self.clock.last_heartbeat = now;
            self.send(
                now,
                Message::Control(Control::Ping {
                    stamp_micros: now.micros(),
                }),
                out,
            );
            self.arm_heartbeat(now, out);
        }

        if self.config.role == Role::Server && self.phase == Phase::Engaged {
            self.flush_pointer_if_due(now, out);
            self.send_snapshot_if_due(now, out);
        }

        self.service_retransmissions(now, out);
        self.send_bare_ack_if_needed(now, out);
    }

    /// Reenvia o que venceu, ou derruba o enlace se alguma mensagem esgotou as tentativas.
    fn service_retransmissions(&mut self, now: Timestamp, out: &mut CommandBatch) {
        let Some(carrier) = self.carrier else { return };
        if !matches!(carrier.delivery(), ir_proto::carrier::Delivery::Datagram) {
            return; // sobre stream o portador já garante entrega
        }

        let timings = self.config.timings;
        let due = self.reliability.on_tick(
            now,
            timings.min_retransmit,
            timings.link_timeout,
            timings.max_retransmits,
        );

        match due {
            Due::Idle => {}
            Due::Retransmit(frames) => {
                for frame in frames {
                    out.push(Command::Send { carrier, frame });
                }
            }
            Due::GiveUp { .. } => {
                // Esgotadas as tentativas, prosseguir seguiria com uma lacuna no canal de
                // teclado. Se a mensagem perdida for um `KeyUp`, a tecla fica presa na
                // máquina do outro — e o usuário não sabe o que aconteceu nem como sair.
                self.tear_down(now, LinkDown::Timeout, out);
            }
        }
    }

    /// Manda uma confirmação pura quando há o que confirmar e nada saindo para carregá-la.
    ///
    /// É o caso da digitação contínua: o servidor manda tecla após tecla e o cliente não tem
    /// nada a dizer. Sem isto, a janela do servidor encheria depois de 64 teclas e a sessão
    /// cairia no meio de uma frase.
    fn send_bare_ack_if_needed(&mut self, now: Timestamp, out: &mut CommandBatch) {
        let Some(carrier) = self.carrier else { return };
        if !matches!(carrier.delivery(), ir_proto::carrier::Delivery::Datagram) {
            return;
        }
        if !now.elapsed_at_least(self.clock.last_bare_ack, ACK_INTERVAL) {
            return;
        }

        // Uma confirmação por canal com algo a confirmar. Um quadro carrega a confirmação de
        // **um** canal, e mandar só a do canal mais urgente deixaria os outros sem
        // confirmação nenhuma — a janela deles encheria e a sessão cairia por um caminho que
        // ninguém associaria à causa.
        let mut sent_any = false;
        for channel in ir_proto::channel::ChannelId::ALL {
            let Some(ack) = self.reliability.ack_for(channel) else {
                continue;
            };
            // Sequência zero e nunca contada: uma confirmação pura não faz parte do fluxo
            // ordenado. Se ela consumisse número de sequência sem ser retransmitida, perder
            // uma criaria um buraco que nunca seria preenchido, e tudo depois dela ficaria
            // esperando para sempre.
            let frame = Frame::new(Message::Control(Control::AckOnly), Sequence::ZERO)
                .with_ack(channel, ack)
                .in_epoch(self.incarnations.local());
            out.push(Command::Send { carrier, frame });
            sent_any = true;
        }
        if sent_any {
            self.clock.last_bare_ack = now;
        }
    }

    pub(super) fn on_pong(&mut self, now: Timestamp, stamp_micros: u64, out: &mut CommandBatch) {
        // O carimbo é nosso e voltou intacto, então a diferença é o tempo de ida e volta —
        // sem precisar de relógio comum entre as máquinas.
        let sent = Timestamp::from_micros(stamp_micros);
        if sent > now {
            return; // carimbo do futuro: só pode ser de outra sessão
        }
        let rtt = now.since(sent);
        self.last_rtt = Some(rtt);
        out.push(Command::Notify(Notice::LatencySample(rtt)));
    }

    pub(super) fn arm_heartbeat(&self, now: Timestamp, out: &mut CommandBatch) {
        out.push(Command::SetTimer {
            id: TimerId::Heartbeat,
            at: now.plus(self.config.timings.heartbeat),
        });
    }

    pub(super) fn arm_link_timeout(&self, now: Timestamp, out: &mut CommandBatch) {
        out.push(Command::SetTimer {
            id: TimerId::LinkTimeout,
            at: now.plus(self.config.timings.link_timeout),
        });
    }
}
