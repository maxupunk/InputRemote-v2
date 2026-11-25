//! Os três portadores, e o que cada um garante.
//!
//! O portador é o meio físico/lógico por onde o quadro viaja. O protocolo é o mesmo nos
//! três — só a camada 0 muda (`docs/03-protocolo.md` §1). Desde a versão 2 a sessão trata todo
//! portador de entrada como datagrama, então o que distingue um do outro aqui é só o que cada um
//! pode carregar e quanto cabe num quadro.

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

impl Carrier {
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

/// Um valor por portador de entrada.
///
/// Não vai para o fio: é como a sessão e o serviço guardam o que vale para cada um dos dois — as
/// vitórias da rota dupla, o último quadro de cada um, quais estão de pé.
///
/// Um campo por portador, e não um vetor: são dois, o conjunto nunca cresce, e um campo nomeado
/// não tem caminho de indexação inválida. O TCP não tem campo — ele não leva entrada, e a sessão
/// não fala por ele —, e perguntar por ele devolve `None` em vez de cair no campo de outro.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PerInputCarrier<T> {
    /// O do Bluetooth.
    pub rfcomm: T,
    /// O da rede.
    pub udp: T,
}

impl<T> PerInputCarrier<T> {
    /// O mesmo valor nos dois.
    pub const fn both(value: T) -> Self
    where
        T: Copy,
    {
        Self {
            rfcomm: value,
            udp: value,
        }
    }

    /// O valor deste portador; `None` para o TCP.
    #[must_use]
    pub const fn get(&self, carrier: Carrier) -> Option<&T> {
        match carrier {
            Carrier::Rfcomm => Some(&self.rfcomm),
            Carrier::Udp => Some(&self.udp),
            Carrier::Tcp => None,
        }
    }

    /// O valor deste portador, para mudar; `None` para o TCP.
    pub const fn get_mut(&mut self, carrier: Carrier) -> Option<&mut T> {
        match carrier {
            Carrier::Rfcomm => Some(&mut self.rfcomm),
            Carrier::Udp => Some(&mut self.udp),
            Carrier::Tcp => None,
        }
    }
}

impl PerInputCarrier<u64> {
    /// Conta mais um para este portador — uma vitória na rota dupla, por exemplo. O TCP não conta.
    pub const fn count(&mut self, carrier: Carrier) {
        if let Some(total) = self.get_mut(carrier) {
            *total = total.saturating_add(1);
        }
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
    fn every_carrier_has_a_positive_frame_limit() {
        for carrier in ALL {
            assert!(carrier.max_plaintext() > limits::MAX_INPUT_MESSAGE);
        }
    }
}
