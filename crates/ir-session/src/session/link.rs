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
use crate::event::{CarrierChoice, Command, CommandBatch, LinkDown, Notice};
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

        // Com a sessão de pé e nada fixado, o portador novo entra na rota sem aperto de mão.
        if self.widen_route(carrier, out) {
            return;
        }
        // Com um aperto de mão em curso e nada fixado, o portador novo entra na rota quando a
        // sessão ficar de pé (`establish`). Recomeçar o aperto de mão por ele só atrasaria a sessão.
        if self.pinned.is_none() && self.route.is_some() && self.phase != Phase::Offline {
            return;
        }

        let Some((chosen, why)) = self.available.pick_input_carrier(self.pinned) else {
            return;
        };

        // Já estamos usando o melhor portador disponível: nada a fazer.
        if self.route.is_some_and(|route| route.uses(chosen)) && self.phase.is_established() {
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

        if !self.route.is_some_and(|route| route.uses(carrier)) {
            return; // caiu um portador que não estava em uso
        }

        // Na rota dupla, o outro portador segue sozinho: nada é solto, nada recomeça. O que estava
        // em trânsito no que caiu chega pelo outro, ou pela retransmissão (`route`).
        if self.narrow_route(carrier, out) {
            return;
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
        self.route = Some(super::Route::Single(carrier));
        self.peer = None;
        self.last_pointer_rx = None;
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

        if let (Some(peer), Some(route)) = (self.peer.as_ref(), self.route) {
            out.push(Command::Notify(Notice::Connected {
                peer: peer.name.clone(),
                carrier: route.primary(),
            }));
        }
        // O aperto de mão foi por um portador; os outros de pé entram na rota agora, e o par fica
        // sabendo por onde mais esta máquina é alcançada.
        self.widen_route_to_available(out);
        self.announce_reach(now, out);
        self.announce_network_power(now, out);
        self.announce_role(now, out);

        // O par precisa do nosso arranjo para saber onde o ponteiro entra.
        if let Some(desktop) = self.local_screens.as_ref() {
            let layout = desktop.to_layout();
            self.send(now, Message::Control(Control::Screens(layout)), out);
        }
        // E a borda, que é do servidor: o cliente passa a usar a oposta (`edge`).
        self.announce_edge(now, out);
    }

    /// Encerra a sessão corrente.
    pub(super) fn tear_down(&mut self, _now: Timestamp, reason: LinkDown, out: &mut CommandBatch) {
        // Avisa o par antes de morrer, quando a decisão é nossa e ainda há por onde falar.
        //
        // Sem isto, quem fica do outro lado só percebe pelo próprio prazo de queda — até um
        // segundo depois — e segura as teclas até lá. O adeus quase sempre chega, porque a
        // maioria das quedas é por perda parcial, não por meio morto. E ele **não** entra na
        // janela de retransmissão: não haveria quem confirmasse, e mandar por `send` poderia
        // reentrar aqui pela janela cheia.
        //
        // Pela rota inteira: o adeus que vai pelos dois portadores é o que tem mais chance de
        // chegar, e a cópia que sobrar é descartada do outro lado como qualquer outra.
        if self.phase.is_established()
            && !matches!(reason, LinkDown::PeerClosed(_) | LinkDown::PeerRestarted)
        {
            let farewell = Frame::new(
                Message::Control(Control::Bye {
                    reason: reason.as_disconnect_reason(),
                }),
                Sequence::ZERO,
            )
            .in_epoch(self.incarnations.local());
            self.dispatch_on_route(farewell, out);
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
        self.route = None;
        self.last_pointer_rx = None;
        self.peer = None;
        self.peer_screens = None;
        self.pending_pointer = ir_proto::input::PointerDelta::ZERO;
        self.seqs.reset();
        self.reliability.reset();
        self.area.reset();
        self.incarnations.forget_peer();
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
        }

        if self.config.role == Role::Server && self.phase == Phase::Engaged {
            self.flush_pointer_if_due(now, out);
            self.send_snapshot_if_due(now, out);
        }

        self.service_retransmissions(now, out);
        self.pump_clipboard(now, out);
        self.send_bare_ack_if_needed(now, out);
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
}
