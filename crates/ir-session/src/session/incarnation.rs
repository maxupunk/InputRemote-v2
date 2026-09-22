//! Encarnações: cada aperto de mão começa uma sessão nova, e quadros de uma sessão que acabou não
//! mexem na seguinte.
//!
//! O defeito que isto corrige apareceu no teste físico por Wi-Fi (log 22). Um pico de latência
//! derrubou a sessão de um lado; o outro continuou mandando quadros com a numeração antiga; o lado
//! que acabara de zerar se ancorou num desses quadros; e o `Hello` novo do par, com número 1,
//! passou a parecer mais velho que a âncora e foi descartado calado. Os dois ficaram reiniciando a
//! sessão a cada ~200 ms, e nada no quadro dizia a qual sessão ele pertencia.
//!
//! Agora todo quadro carrega a [`Epoch`] de quem o enviou, e quem recebe decide antes de qualquer
//! outra coisa:
//!
//! | O quadro é | E a época é | Então |
//! |---|---|---|
//! | qualquer um | a da sessão corrente do par | segue o caminho normal |
//! | `Hello` ou `HelloAck` | nova | o par começou outra sessão: acompanhá-lo |
//! | `Hello` ou `HelloAck` | uma já aposentada | eco atrasado de uma sessão que acabou: descartar |
//! | qualquer outro | diferente da corrente | resto de outra sessão: descartar |
//!
//! Só um aperto de mão pode inaugurar uma encarnação. Um `Ping` velho não ressuscita uma sessão
//! desligada, um `Bye` velho não derruba a nova, e um `KeyDown` velho nunca vira tecla digitada.

use ir_proto::carrier::Carrier;
use ir_proto::frame::{Epoch, Frame};
use ir_proto::message::{Control, Message};

use crate::event::{CommandBatch, LinkDown};
use crate::phase::Phase;
use crate::session::{Clock, Session};
use crate::time::Timestamp;

/// Quantas épocas do par ficam lembradas depois de aposentadas.
///
/// A rede duplica e atrasa: um `Hello` de duas sessões atrás ainda pode chegar. Quatro cobre com
/// folga as reinicializações que cabem no tempo de vida de um datagrama esquecido na rede.
const RETIRED: usize = 4;

/// O que fazer com um quadro, conforme a encarnação a que ele pertence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Admission {
    /// É da sessão corrente do par.
    Current,
    /// É o aperto de mão de uma sessão nova do par.
    NewPeer,
    /// É de uma sessão que já acabou.
    Stale,
}

/// A encarnação desta ponta e a do par.
#[derive(Debug, Clone, Default)]
pub(crate) struct Incarnations {
    seed: u32,
    started: u32,
    local: Epoch,
    peer: Option<Epoch>,
    retired: [Option<Epoch>; RETIRED],
    next_retired: usize,
}

impl Incarnations {
    /// Nenhuma encarnação começada, com as épocas nascendo de `seed`.
    pub(crate) fn new(seed: u32) -> Self {
        Self {
            seed,
            local: Epoch(seed),
            ..Self::default()
        }
    }

    /// A época com que esta ponta marca o que manda.
    pub(crate) const fn local(&self) -> Epoch {
        self.local
    }

    /// Começa uma encarnação desta ponta, com uma época que nenhuma anterior desta sessão usou.
    pub(crate) const fn start_local(&mut self) {
        self.local = Epoch(self.seed.wrapping_add(self.started));
        self.started = self.started.wrapping_add(1);
    }

    /// Esquece a sessão corrente do par, aposentando a época dela.
    pub(crate) fn forget_peer(&mut self) {
        if let Some(epoch) = self.peer.take() {
            if let Some(slot) = self.retired.get_mut(self.next_retired) {
                *slot = Some(epoch);
            }
            self.next_retired = (self.next_retired + 1) % RETIRED;
        }
    }

    /// Passa a acompanhar a sessão do par que tem esta época.
    fn follow_peer(&mut self, epoch: Epoch) {
        self.forget_peer();
        self.peer = Some(epoch);
    }

    /// A qual encarnação este quadro pertence.
    fn admit(&self, frame: &Frame) -> Admission {
        if self.peer == Some(frame.epoch) {
            Admission::Current
        } else if is_greeting(frame) && !self.retired.contains(&Some(frame.epoch)) {
            Admission::NewPeer
        } else {
            Admission::Stale
        }
    }
}

/// Se o quadro é um aperto de mão — a única coisa que pode inaugurar uma encarnação.
fn is_greeting(frame: &Frame) -> bool {
    matches!(
        frame.message,
        Message::Control(Control::Hello(_) | Control::HelloAck(_))
    )
}

impl Session {
    /// Decide se um quadro recebido pertence à sessão, e acompanha o par quando ele começou outra.
    ///
    /// Devolve `false` quando o quadro deve ser descartado sem mais nada: nem prova de vida, nem
    /// confirmação, nem adoção de portador.
    pub(super) fn admit(
        &mut self,
        now: Timestamp,
        carrier: Carrier,
        frame: &Frame,
        out: &mut CommandBatch,
    ) -> bool {
        match self.incarnations.admit(frame) {
            Admission::Current => true,
            Admission::Stale => false,
            Admission::NewPeer => {
                self.follow_new_peer(now, carrier, frame.epoch, out);
                true
            }
        }
    }

    fn follow_new_peer(
        &mut self,
        now: Timestamp,
        carrier: Carrier,
        epoch: Epoch,
        out: &mut CommandBatch,
    ) {
        if self.phase.is_established() {
            // O par reiniciou sem que soubéssemos — o adeus dele se perdeu. A sessão daqui é de
            // uma encarnação que ele já abandonou: encerrá-la, soltando tudo, e recomeçar junto.
            self.tear_down(now, LinkDown::PeerRestarted, out);
        }
        if self.phase == Phase::Offline {
            if carrier.carries_input() {
                // Quem ouve primeiro, responde — mas numa encarnação própria, do zero. Sem isto a
                // resposta sairia com a numeração e a época de uma sessão que já acabou.
                self.available.set(carrier, true);
                self.route = Some(super::Route::Single(carrier));
                self.last_pointer_rx = None;
                self.seqs.reset();
                self.reliability.reset();
                self.clock = Clock::started_at(now);
                self.incarnations.start_local();
                self.move_to(Phase::Handshaking, out);
            }
        } else {
            // No meio do nosso aperto de mão: o que o par mandou até aqui era de outra sessão dele.
            self.reliability.reset_receivers();
            self.last_pointer_rx = None;
        }
        self.incarnations.follow_peer(epoch);
    }
}

#[cfg(test)]
mod tests {
    use ir_proto::frame::Sequence;
    use ir_proto::ids::MachineId;
    use ir_proto::message::Greeting;
    use ir_proto::peer::{Capabilities, MachineName};
    use ir_proto::version;

    use super::*;

    fn ping(epoch: u32) -> Frame {
        Frame::new(
            Message::Control(Control::Ping { stamp_micros: 0 }),
            Sequence(3),
        )
        .in_epoch(Epoch(epoch))
    }

    fn hello(epoch: u32) -> Frame {
        let greeting = Greeting {
            version: version::CURRENT,
            machine: MachineId([1; 16]),
            name: MachineName::coagido("par"),
            capabilities: Capabilities::default(),
        };
        Frame::new(Message::Control(Control::Hello(greeting)), Sequence(1)).in_epoch(Epoch(epoch))
    }

    #[test]
    fn each_local_incarnation_gets_a_distinct_epoch_even_across_the_wraparound() {
        let mut incarnations = Incarnations::new(u32::MAX - 1);
        let mut seen = Vec::new();
        for _ in 0..4 {
            incarnations.start_local();
            seen.push(incarnations.local());
        }
        for (index, epoch) in seen.iter().enumerate() {
            assert!(
                !seen.iter().skip(index + 1).any(|other| other == epoch),
                "época repetida entre encarnações: {seen:?}"
            );
        }
    }

    #[test]
    fn only_a_greeting_opens_a_new_incarnation() {
        let incarnations = Incarnations::new(0);
        assert_eq!(
            incarnations.admit(&ping(9)),
            Admission::Stale,
            "um Ping não inaugura sessão"
        );
        assert_eq!(incarnations.admit(&hello(9)), Admission::NewPeer);
    }

    #[test]
    fn a_retired_epoch_stays_retired() {
        let mut incarnations = Incarnations::new(0);
        incarnations.follow_peer(Epoch(5));
        assert_eq!(incarnations.admit(&ping(5)), Admission::Current);

        incarnations.follow_peer(Epoch(6));
        assert_eq!(incarnations.admit(&ping(5)), Admission::Stale);
        assert_eq!(
            incarnations.admit(&hello(5)),
            Admission::Stale,
            "um Hello de uma sessão que acabou não é sessão nova"
        );
        assert_eq!(incarnations.admit(&ping(6)), Admission::Current);
    }

    #[test]
    fn the_retired_memory_is_bounded_and_forgets_the_oldest() {
        let mut incarnations = Incarnations::new(0);
        for epoch in 1..=6 {
            incarnations.follow_peer(Epoch(epoch));
        }
        // Aposentadas, das mais novas às mais velhas: 5, 4, 3, 2. A 1 já saiu da memória.
        assert_eq!(incarnations.admit(&hello(2)), Admission::Stale);
        assert_eq!(incarnations.admit(&hello(1)), Admission::NewPeer);
    }
}
