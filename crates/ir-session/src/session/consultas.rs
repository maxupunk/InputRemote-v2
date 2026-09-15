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

use super::{PeerInfo, Session};
use crate::config::Role;
use crate::phase::Phase;
use crate::time::Millis;

impl Session {
    /// Em que ponto a sessão está.
    #[must_use]
    pub const fn phase(&self) -> Phase {
        self.phase
    }

    /// O portador de entrada em uso.
    #[must_use]
    pub const fn carrier(&self) -> Option<Carrier> {
        self.carrier
    }

    /// O par, depois do handshake.
    #[must_use]
    pub const fn peer(&self) -> Option<&PeerInfo> {
        self.peer.as_ref()
    }

    /// O papel desta máquina.
    #[must_use]
    pub const fn role(&self) -> Role {
        self.config.role
    }

    /// A borda desta tela que dá para a tela do par — a única que atravessa.
    ///
    /// No servidor é a escolhida pelo usuário; no cliente é a oposta da que o servidor anunciou.
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
