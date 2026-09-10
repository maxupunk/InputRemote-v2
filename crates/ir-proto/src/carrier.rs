//! Os três portadores, e o que cada um garante.
//!
//! O portador é o meio físico/lógico por onde o quadro viaja. O protocolo é o mesmo nos
//! três — só a camada 0 muda (`docs/03-protocolo.md` §1). Este módulo existe para que o
//! resto do produto raciocine sobre "stream confiável" ou "datagrama sem garantia" em vez
//! de sobre RFCOMM, UDP ou TCP.

use serde::{Deserialize, Serialize};

use crate::limits;

/// Por onde um quadro viaja.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Carrier {
    /// Bluetooth RFCOMM. Stream confiável e ordenado, latência constante, banda pequena.
    Rfcomm,
    /// UDP na rede local. Datagrama sem garantia nenhuma, latência menor, banda grande.
    Udp,
    /// TCP na rede local. Stream confiável, só para clipboard grande, imagens e arquivos.
    Tcp,
}

/// O que o meio garante por si, antes de qualquer coisa que a aplicação acrescente.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Delivery {
    /// Entrega confiável e ordenada pelo próprio meio.
    ReliableStream,
    /// Sem garantia: pode perder, duplicar e reordenar.
    Datagram,
}

impl Carrier {
    /// O que este portador garante por si.
    ///
    /// Sobre [`Delivery::ReliableStream`], a confiabilidade dos canais é grátis. Sobre
    /// [`Delivery::Datagram`], ela é responsabilidade da aplicação
    /// (`docs/03-protocolo.md` §4.1).
    #[must_use]
    pub const fn delivery(self) -> Delivery {
        match self {
            Self::Rfcomm | Self::Tcp => Delivery::ReliableStream,
            Self::Udp => Delivery::Datagram,
        }
    }

    /// Máximo de texto claro que cabe num quadro deste portador.
    ///
    /// Para [`Carrier::Rfcomm`] este é o teto seguro; o valor efetivo é negociado no enlace
    /// e pode ser menor.
    #[must_use]
    pub const fn max_plaintext(self) -> usize {
        match self {
            Self::Rfcomm => limits::MAX_RFCOMM_PLAINTEXT,
            Self::Udp => limits::MAX_UDP_PLAINTEXT,
            Self::Tcp => limits::MAX_TCP_PLAINTEXT,
        }
    }

    /// Se este portador serve para teclado e mouse.
    ///
    /// TCP não serve, e a proibição é dura: o controle de congestionamento e o bloqueio de
    /// cabeça de fila do TCP transformam uma retransmissão em atraso visível no ponteiro.
    #[must_use]
    pub const fn carries_input(self) -> bool {
        matches!(self, Self::Rfcomm | Self::Udp)
    }

    /// Se este portador serve para imagens e arquivos.
    ///
    /// Só TCP. Um arquivo grande no rádio Bluetooth destrói a latência da entrada, que é a
    /// única coisa que o Bluetooth está ali para fazer bem.
    #[must_use]
    pub const fn carries_bulk(self) -> bool {
        matches!(self, Self::Tcp)
    }

    /// Nome curto e estável, para log e interface.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Rfcomm => "bluetooth",
            Self::Udp => "udp",
            Self::Tcp => "tcp",
        }
    }
}

impl core::fmt::Display for Carrier {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.name())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: [Carrier; 3] = [Carrier::Rfcomm, Carrier::Udp, Carrier::Tcp];

    #[test]
    fn input_never_travels_on_tcp() {
        assert!(!Carrier::Tcp.carries_input());
    }

    #[test]
    fn bulk_travels_only_on_tcp() {
        for carrier in ALL {
            assert_eq!(carrier.carries_bulk(), carrier == Carrier::Tcp);
        }
    }

    #[test]
    fn input_and_bulk_never_share_a_carrier() {
        // A separação é o requisito R2 de docs/01-visao-e-escopo.md: transferência de
        // arquivo não compartilha portador com entrada, nunca.
        for carrier in ALL {
            assert!(!(carrier.carries_input() && carrier.carries_bulk()));
        }
    }

    #[test]
    fn only_udp_is_unreliable() {
        for carrier in ALL {
            let expected = if carrier == Carrier::Udp {
                Delivery::Datagram
            } else {
                Delivery::ReliableStream
            };
            assert_eq!(carrier.delivery(), expected);
        }
    }

    #[test]
    fn every_carrier_has_a_positive_frame_limit() {
        for carrier in ALL {
            assert!(carrier.max_plaintext() > limits::MAX_INPUT_MESSAGE);
        }
    }
}
