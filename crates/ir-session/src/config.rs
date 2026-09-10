//! Configuração e prazos da sessão.
//!
//! Todo número de tempo do produto está aqui, com a origem documentada. Espalhar prazos pelo
//! código é como se perde a coerência entre eles — e a coerência importa: o intervalo de
//! *heartbeat* precisa ser bem menor que o prazo de queda, senão a sessão cai sozinha.
//! [`Timings::is_coherent`] verifica isso, e há teste exigindo que o padrão passe.

use ir_proto::screens::Edge;

use crate::time::Millis;

/// O papel desta máquina na sessão.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Role {
    /// Tem o teclado e o mouse físicos. Captura e envia.
    Server,
    /// É controlada. Recebe e injeta.
    Client,
}

impl Role {
    /// O papel da outra ponta.
    #[must_use]
    pub const fn peer(self) -> Self {
        match self {
            Self::Server => Self::Client,
            Self::Client => Self::Server,
        }
    }

    /// Se este papel captura entrada local.
    #[must_use]
    pub const fn captures(self) -> bool {
        matches!(self, Self::Server)
    }

    /// Se este papel injeta entrada.
    #[must_use]
    pub const fn injects(self) -> bool {
        matches!(self, Self::Client)
    }

    /// Nome estável, para interface e log.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Server => "servidor",
            Self::Client => "cliente",
        }
    }
}

impl core::fmt::Display for Role {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.name())
    }
}

/// Os prazos do produto.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Timings {
    /// De quanto em quanto tempo se manda `Ping`.
    ///
    /// Origem: precisa ser bem menor que [`Self::link_timeout`] para que uma perda isolada
    /// não derrube a sessão.
    pub heartbeat: Millis,

    /// Sem nenhuma mensagem do par por este tempo, o enlace é declarado caído.
    ///
    /// Origem: `docs/01-visao-e-escopo.md` §6 — retorno do controle em até 1 s.
    pub link_timeout: Millis,

    /// De quanto em quanto tempo o servidor manda o estado completo enquanto controla.
    ///
    /// Origem: `docs/03-protocolo.md` §7.
    pub snapshot_interval: Millis,

    /// Tempo mínimo entre duas amostras de ponteiro enviadas.
    ///
    /// Origem: `docs/01-visao-e-escopo.md` §6 — pelo menos 125 Hz úteis, que é uma amostra a
    /// cada 8 ms. As amostras que chegarem no meio são coalescidas, não descartadas.
    pub pointer_interval: Millis,

    /// Quanto esperar por confirmação antes de retransmitir, no piso.
    ///
    /// Origem: `docs/03-protocolo.md` §4.1 — `RTO = max(20 ms, 2 × srtt)`.
    pub min_retransmit: Millis,

    /// Quantas retransmissões antes de derrubar o enlace.
    ///
    /// Origem: `docs/03-protocolo.md` §4.1. Esgotado o limite, cai — não se prossegue com
    /// lacuna, porque um `KeyUp` perdido é uma tecla presa.
    pub max_retransmits: u8,

    /// Quanto esperar antes de tentar reconectar.
    ///
    /// Origem: `docs/01-visao-e-escopo.md` §6 — reconexão em até 5 s.
    pub reconnect_delay: Millis,
}

impl Timings {
    /// Os prazos padrão do produto.
    pub const DEFAULT: Self = Self {
        heartbeat: Millis(200),
        link_timeout: Millis(1000),
        snapshot_interval: Millis(250),
        pointer_interval: Millis(8),
        min_retransmit: Millis(20),
        max_retransmits: 5,
        reconnect_delay: Millis(1000),
    };

    /// Se os prazos fazem sentido entre si.
    ///
    /// Não é firula. Um `heartbeat` maior que o `link_timeout` faz a sessão cair sozinha a
    /// cada ciclo, e o sintoma seria "desconecta sozinho de vez em quando" — o tipo de
    /// defeito que se persegue por semanas.
    #[must_use]
    pub const fn is_coherent(&self) -> bool {
        self.heartbeat.get() > 0
            && self.pointer_interval.get() > 0
            && self.max_retransmits > 0
            // Ao menos três batidas cabem antes de declarar queda.
            && self.heartbeat.times(3).get() <= self.link_timeout.get()
            // O snapshot não pode ser mais raro que o prazo de queda: ele é a rede de
            // segurança contra tecla presa, e chegar depois da queda não serve.
            && self.snapshot_interval.get() <= self.link_timeout.get()
            // Retransmitir todas as tentativas tem de caber antes da queda.
            && self.min_retransmit.times(self.max_retransmits as u32).get()
                <= self.link_timeout.get()
    }
}

impl Default for Timings {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// A configuração de uma sessão.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SessionConfig {
    /// O papel desta máquina.
    pub role: Role,
    /// A borda desta tela que dá para a tela do par.
    ///
    /// Só ela atravessa. As outras três prendem o ponteiro
    /// (`ir_geometry::crossing`).
    pub peer_edge: Edge,
    /// Os prazos.
    pub timings: Timings,
}

impl SessionConfig {
    /// Uma configuração de servidor com o par à direita e prazos padrão.
    #[must_use]
    pub const fn server(peer_edge: Edge) -> Self {
        Self {
            role: Role::Server,
            peer_edge,
            timings: Timings::DEFAULT,
        }
    }

    /// Uma configuração de cliente.
    ///
    /// A borda é a que dá de volta para o servidor: se o cliente está à direita, o servidor
    /// fica à esquerda dele.
    #[must_use]
    pub const fn client(peer_edge: Edge) -> Self {
        Self {
            role: Role::Client,
            peer_edge,
            timings: Timings::DEFAULT,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roles_are_complementary() {
        assert_eq!(Role::Server.peer(), Role::Client);
        assert_eq!(Role::Client.peer(), Role::Server);
        assert_eq!(Role::Server.peer().peer(), Role::Server);
    }

    #[test]
    fn only_the_server_captures_and_only_the_client_injects() {
        assert!(Role::Server.captures());
        assert!(!Role::Server.injects());
        assert!(Role::Client.injects());
        assert!(
            !Role::Client.captures(),
            "capturar no cliente seria ler a senha da máquina"
        );
    }

    #[test]
    fn the_default_timings_are_coherent() {
        assert!(Timings::DEFAULT.is_coherent());
        assert!(Timings::default().is_coherent());
    }

    #[test]
    fn a_heartbeat_slower_than_the_timeout_is_refused() {
        let bad = Timings {
            heartbeat: Millis(2000),
            ..Timings::DEFAULT
        };
        assert!(!bad.is_coherent(), "a sessão cairia sozinha a cada ciclo");
    }

    #[test]
    fn a_snapshot_rarer_than_the_timeout_is_refused() {
        let bad = Timings {
            snapshot_interval: Millis(5000),
            ..Timings::DEFAULT
        };
        assert!(!bad.is_coherent(), "o snapshot chegaria depois da queda");
    }

    #[test]
    fn retransmissions_must_fit_before_the_link_is_declared_dead() {
        let bad = Timings {
            min_retransmit: Millis(500),
            max_retransmits: 5,
            ..Timings::DEFAULT
        };
        assert!(!bad.is_coherent(), "5 × 500 ms passa do prazo de 1 s");
    }

    #[test]
    fn zeroed_intervals_are_refused() {
        for bad in [
            Timings {
                heartbeat: Millis::ZERO,
                ..Timings::DEFAULT
            },
            Timings {
                pointer_interval: Millis::ZERO,
                ..Timings::DEFAULT
            },
            Timings {
                max_retransmits: 0,
                ..Timings::DEFAULT
            },
        ] {
            assert!(!bad.is_coherent(), "{bad:?}");
        }
    }

    #[test]
    fn the_pointer_interval_allows_at_least_125_hz() {
        assert!(
            Timings::DEFAULT.pointer_interval.get() <= 8,
            "125 Hz é uma amostra a cada 8 ms"
        );
    }

    #[test]
    fn constructors_set_the_expected_role() {
        assert_eq!(SessionConfig::server(Edge::Right).role, Role::Server);
        assert_eq!(SessionConfig::client(Edge::Left).role, Role::Client);
        assert_eq!(SessionConfig::server(Edge::Top).peer_edge, Edge::Top);
    }
}
