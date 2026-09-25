//! O que se pode perguntar à sessão — sem mudar nada nela.
//!
//! Separado do despacho de eventos ([`super`]) por responsabilidade: aqui só há leitura. A
//! periferia (o serviço, o diagnóstico, os testes) observa a sessão por estas consultas e nunca
//! pelos campos, e é isso que deixa o estado interno livre para mudar sem quebrar quem observa.

use ir_geometry::Point;
use ir_proto::carrier::Carrier;
use ir_proto::channel::ChannelId;
use ir_proto::frame::Sequence;
use ir_proto::input::InputState;
use ir_proto::screens::Edge;

use super::{CarrierWins, PeerInfo, Route, RouteReport, Session};
use crate::config::Policy;
use crate::phase::Phase;
use crate::time::{Millis, Timestamp};

impl Session {
    /// Em que ponto a sessão está.
    #[must_use]
    pub const fn phase(&self) -> Phase {
        self.phase
    }

    /// O portador de entrada que representa a rota em uso — o Bluetooth, na rota dupla.
    #[must_use]
    pub const fn carrier(&self) -> Option<Carrier> {
        match self.route {
            Some(route) => Some(route.primary()),
            None => None,
        }
    }

    /// Por quais portadores a sessão fala agora.
    #[must_use]
    pub const fn route(&self) -> Option<Route> {
        self.route
    }

    /// Por qual portador cada quadro novo chegou primeiro, desde que o serviço subiu.
    #[must_use]
    pub const fn carrier_wins(&self) -> CarrierWins {
        self.wins
    }

    /// O retrato da rota agora, para o registro e o diagnóstico.
    #[must_use]
    pub fn route_report(&self, now: Timestamp) -> RouteReport {
        RouteReport {
            route: self.route,
            wins: self.wins,
            silence_rfcomm: self.carrier_silence(Carrier::Rfcomm, now),
            silence_udp: self.carrier_silence(Carrier::Udp, now),
        }
    }

    /// Há quanto tempo não chega nada por este portador da rota — `None` se ele não está nela.
    ///
    /// É o que deixa a interface dizer qual dos dois portadores da rota dupla parou de responder
    /// antes de o transporte perceber a queda.
    #[must_use]
    pub fn carrier_silence(&self, carrier: Carrier, now: Timestamp) -> Option<Millis> {
        let last = self.clock.carrier_rx.get(carrier)?;
        self.route?.uses(carrier).then(|| now.since(*last))
    }

    /// O par, depois do handshake.
    #[must_use]
    pub const fn peer(&self) -> Option<&PeerInfo> {
        self.peer.as_ref()
    }

    /// Quem pode controlar quem, nesta sessão.
    #[must_use]
    pub const fn policy(&self) -> Policy {
        self.config.policy
    }

    /// A borda desta tela que dá para a tela do par — a única que atravessa.
    ///
    /// A escolhida nesta tela, ou a oposta da que o par escolheu por último (`session/edge.rs`).
    /// Exposta para quem troca a borda poder confirmar que a sessão **em uso** é a da borda nova:
    /// a periferia já mostrou uma borda enquanto a sessão atravessava por outra.
    #[must_use]
    pub const fn peer_edge(&self) -> Edge {
        self.config.peer_edge
    }

    /// O que está pressionado, do ponto de vista desta máquina.
    #[must_use]
    pub const fn input_state(&self) -> &InputState {
        &self.input_state
    }

    /// Onde o ponteiro está nesta máquina.
    #[must_use]
    pub const fn pointer(&self) -> Point {
        self.pointer
    }

    /// Onde o ponteiro está, normalizado no monitor em que está — a forma que o injetor absoluto
    /// recebe. `None` sem arranjo de telas.
    #[must_use]
    pub fn pointer_position(&self) -> Option<ir_proto::input::PointerPosition> {
        self.local_screens
            .as_ref()
            .map(|desktop| desktop.to_position(self.pointer))
    }

    /// A posição do ponteiro em coordenadas cruas, para a periferia que não conhece `Point`.
    #[must_use]
    pub const fn pointer_xy(&self) -> (i32, i32) {
        (self.pointer.x, self.pointer.y)
    }

    /// A última ida e volta medida até o par, se já houve alguma.
    #[must_use]
    pub const fn last_rtt(&self) -> Option<Millis> {
        self.last_rtt
    }

    /// A sequência que este canal usaria em seguida. Só para teste e diagnóstico.
    #[must_use]
    pub const fn next_sequence(&self, channel: ChannelId) -> Sequence {
        self.seqs.peek(channel)
    }
}
