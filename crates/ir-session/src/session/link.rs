//! Enlace: subida, handshake, prazos e queda.
//!
//! A regra que governa este módulo: **em toda queda, solta-se tudo antes de qualquer outra
//! coisa.** Um enlace que morre com `Ctrl` pressionado do outro lado deixa a máquina do
//! usuário inutilizável até o próximo reinício, e é a falha mais cara que este produto pode
//! cometer (`docs/02-arquitetura.md` §8).

use ir_proto::carrier::Carrier;
use ir_proto::message::{Control, DisconnectReason, ErrorCode, Greeting, Message};
use ir_proto::version;

use crate::config::Role;
use crate::event::{CarrierChoice, Command, CommandBatch, LinkDown, Notice, TimerId};
use crate::phase::Phase;
use crate::session::Session;
use crate::session::state::{Clock, PeerInfo};
use crate::time::Timestamp;

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

    /// Encerra por vontade própria, avisando o par antes.
    pub fn stop(&mut self, now: Timestamp, reason: LinkDown, out: &mut CommandBatch) {
        if self.phase.is_established() {
            self.send(
                now,
                Message::Control(Control::Bye {
                    reason: reason.as_disconnect_reason(),
                }),
                out,
            );
        }
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
