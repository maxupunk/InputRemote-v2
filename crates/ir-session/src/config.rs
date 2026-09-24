//! Configuração e prazos da sessão.
//!
//! Todo número de tempo do produto está aqui, com a origem documentada. Espalhar prazos pelo
//! código é como se perde a coerência entre eles — e a coerência importa: o intervalo de
//! *heartbeat* precisa ser bem menor que o prazo de queda, senão a sessão cai sozinha.
//! [`Timings::is_coherent`] verifica isso, e há teste exigindo que o padrão passe.

use ir_proto::screens::Edge;

use crate::time::Millis;

/// Quem pode controlar quem ([ADR-0014](../../../docs/adr/0014-controle-simetrico.md)).
///
/// Não é um papel: com [`Policy::Both`], o padrão, qualquer um dos dois computadores leva o controle
/// ao outro, e quem está usando agora é só a fase da sessão. As outras duas existem para quem precisa
/// de um computador que nunca comanda, ou que nunca é comandado.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Policy {
    /// Os dois controlam um ao outro.
    #[default]
    Both,
    /// Este computador controla o outro, e nunca é controlado.
    OnlyControls,
    /// Este computador é controlado pelo outro, e nunca o controla.
    OnlyControlled,
}

impl Policy {
    /// Se a entrada daqui pode ir para o par.
    #[must_use]
    pub const fn sends(self) -> bool {
        !matches!(self, Self::OnlyControlled)
    }

    /// Se esta máquina aceita ser controlada pelo par.
    #[must_use]
    pub const fn receives(self) -> bool {
        !matches!(self, Self::OnlyControls)
    }

    /// Nome estável, para interface e log.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Both => "os dois",
            Self::OnlyControls => "só este controla",
            Self::OnlyControlled => "só o outro controla",
        }
    }
}

impl core::fmt::Display for Policy {
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
    /// Origem: `docs/03-protocolo.md` §4.1 — `RTO = max(20 ms, 2 × srtt)`, dobrando a cada
    /// reenvio. Não há contagem de tentativas: o enlace cai quando uma mensagem passa de
    /// [`Self::link_timeout`] sem confirmação, contado do primeiro envio (log 23).
    pub min_retransmit: Millis,

    /// Depois de o par levar o controle para cá, por quanto tempo o teclado e o mouse daqui não o
    /// retomam.
    ///
    /// Origem: ADR-0014 — é o intervalo em que a mão de quem atravessou ainda está chegando, e em
    /// que um esbarrão na mesa daqui devolveria o controle sem ninguém querer.
    pub reclaim_grace: Millis,

    /// Em quanto tempo o ponteiro daqui precisa andar [`crate::session::RECLAIM_DISTANCE`] para
    /// retomar o controle. Mais devagar que isso é tremida, não gesto.
    ///
    /// Origem: ADR-0014.
    pub reclaim_window: Millis,
}

impl Timings {
    /// Os prazos padrão do produto.
    pub const DEFAULT: Self = Self {
        heartbeat: Millis(200),
        link_timeout: Millis(1000),
        snapshot_interval: Millis(250),
        pointer_interval: Millis(8),
        min_retransmit: Millis(20),
        reclaim_grace: Millis(150),
        reclaim_window: Millis(300),
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
            && self.min_retransmit.get() > 0
            // Ao menos três batidas cabem antes de declarar queda.
            && self.heartbeat.times(3).get() <= self.link_timeout.get()
            // O snapshot não pode ser mais raro que o prazo de queda: ele é a rede de
            // segurança contra tecla presa, e chegar depois da queda não serve.
            && self.snapshot_interval.get() <= self.link_timeout.get()
            // Cabem ao menos quatro reenvios no piso antes da queda: com menos que isso, um pico
            // de latência vira queda antes de a retransmissão ter tido chance de reparar.
            && self.min_retransmit.times(4).get() <= self.link_timeout.get()
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
    /// Quem pode controlar quem.
    pub policy: Policy,
    /// A borda desta tela que dá para a tela do par.
    ///
    /// Só ela atravessa. As outras três prendem o ponteiro (`ir_geometry::crossing`). O par usa a
    /// oposta; se as duas divergirem, vale a escolhida mais recentemente (`session/edge.rs`).
    pub peer_edge: Edge,
    /// Quando [`Self::peer_edge`] foi escolhida na tela, em milissegundos desde 1970; `0` se nunca
    /// foi.
    pub edge_chosen_at: u64,
    /// Os prazos.
    pub timings: Timings,
    /// De onde nascem as épocas desta sessão ([`ir_proto::frame::Epoch`]).
    ///
    /// Cada aperto de mão começa uma encarnação nova, e cada quadro carrega a época dela, para
    /// que o par descarte o que sobrou de uma sessão que já acabou. O núcleo não sorteia nada
    /// ([ADR-0004](../../../docs/adr/0004-nucleo-sans-io.md)), então a semente vem de fora: o
    /// serviço sorteia uma a cada sessão criada. Repetir a semente entre duas execuções do
    /// serviço faria o par tomar a sessão nova pela antiga — o laço que ela existe para impedir.
    pub incarnation_seed: u32,
}

impl SessionConfig {
    /// Os dois controlando um ao outro, com o par do lado dado e prazos padrão.
    #[must_use]
    pub const fn new(peer_edge: Edge) -> Self {
        Self {
            policy: Policy::Both,
            peer_edge,
            edge_chosen_at: 0,
            timings: Timings::DEFAULT,
            incarnation_seed: 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn by_default_both_control_each_other() {
        let policy = Policy::default();
        assert!(policy.sends() && policy.receives());
    }

    #[test]
    fn the_restricted_policies_close_exactly_one_direction() {
        assert!(Policy::OnlyControls.sends());
        assert!(!Policy::OnlyControls.receives());
        assert!(Policy::OnlyControlled.receives());
        assert!(!Policy::OnlyControlled.sends());
    }

    #[test]
    fn the_grace_is_shorter_than_the_reclaim_window() {
        let t = Timings::DEFAULT;
        assert!(t.reclaim_grace.get() < t.reclaim_window.get());
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
            ..Timings::DEFAULT
        };
        assert!(
            !bad.is_coherent(),
            "com piso de 500 ms não cabem quatro reenvios antes do prazo de 1 s"
        );
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
                min_retransmit: Millis::ZERO,
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
    fn a_new_session_lets_both_control_each_other() {
        let config = SessionConfig::new(Edge::Top);
        assert_eq!(config.policy, Policy::Both);
        assert_eq!(config.peer_edge, Edge::Top);
        assert_eq!(config.edge_chosen_at, 0, "nunca escolhida pela tela");
    }
}
