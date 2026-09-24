//! A rota: por quais portadores de entrada uma sessão fala — um só, ou os dois da rota dupla.
//!
//! Um tipo de valor sobre [`Carrier`], sem estado de sessão: o que a sessão faz com a rota — alargar,
//! estreitar, despachar um quadro por ela — mora em `ir-session`
//! ([ADR-0012](../../../docs/adr/0012-rota-dupla.md)).

use crate::carrier::Carrier;

/// Por quais portadores de entrada a sessão fala.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Route {
    /// Um portador só.
    Single(Carrier),
    /// Bluetooth e rede ao mesmo tempo; vale o que chegar primeiro.
    Dual,
}

impl Route {
    /// Os dois portadores de entrada, na ordem de preferência.
    pub const INPUT: [Carrier; 2] = [Carrier::Rfcomm, Carrier::Udp];

    /// O portador que representa a rota numa frase só.
    ///
    /// Na rota dupla é o Bluetooth, o preferido da política única: quem precisa de **um** nome —
    /// o aviso de conexão, o estado antigo da interface — continua recebendo o mesmo de antes.
    #[must_use]
    pub const fn primary(self) -> Carrier {
        match self {
            Self::Single(carrier) => carrier,
            Self::Dual => Carrier::Rfcomm,
        }
    }

    /// Se a rota passa por este portador.
    #[must_use]
    pub const fn uses(self, carrier: Carrier) -> bool {
        match self {
            Self::Single(own) => matches!(
                (own, carrier),
                (Carrier::Rfcomm, Carrier::Rfcomm)
                    | (Carrier::Udp, Carrier::Udp)
                    | (Carrier::Tcp, Carrier::Tcp)
            ),
            Self::Dual => carrier.carries_input(),
        }
    }

    /// Se a rota é dupla.
    #[must_use]
    pub const fn is_dual(self) -> bool {
        matches!(self, Self::Dual)
    }

    /// Os portadores da rota, na ordem de preferência.
    pub fn carriers(self) -> impl Iterator<Item = Carrier> {
        Self::INPUT
            .into_iter()
            .filter(move |carrier| self.uses(*carrier))
    }

    /// A rota com este portador a mais.
    #[must_use]
    pub const fn with(self, carrier: Carrier) -> Self {
        if self.uses(carrier) || !carrier.carries_input() {
            self
        } else {
            Self::Dual
        }
    }

    /// A rota sem este portador, ou nada se ele era o último.
    #[must_use]
    pub const fn without(self, carrier: Carrier) -> Option<Self> {
        match self {
            Self::Dual => match carrier {
                Carrier::Rfcomm => Some(Self::Single(Carrier::Udp)),
                Carrier::Udp => Some(Self::Single(Carrier::Rfcomm)),
                Carrier::Tcp => Some(Self::Dual),
            },
            Self::Single(_) if self.uses(carrier) => None,
            Self::Single(_) => Some(self),
        }
    }
}

impl core::fmt::Display for Route {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Single(carrier) => write!(f, "{carrier}"),
            Self::Dual => f.write_str("bluetooth+udp"),
        }
    }
}
