//! Os seis canais lógicos, e a política de o que pode viajar por onde.
//!
//! A política vive aqui, como dado consultável, e não espalhada pelas camadas de
//! transporte. Quem transporta pergunta; não decide. É a inversão de dependência de
//! `docs/02-arquitetura.md` §2 aplicada em pequeno.
//!
//! Tabela de referência: `docs/03-protocolo.md` §4.

use serde::{Deserialize, Serialize};

use crate::carrier::{Carrier, Delivery};
use crate::error::{ChannelName, ProtoError, Result};

/// Canal lógico de um quadro.
///
/// O discriminante é o primeiro byte do texto claro e **é estável**: mudar um valor destes
/// quebra a compatibilidade de fio e exige incremento de `version::CURRENT`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[repr(u8)]
pub enum ChannelId {
    /// Handshake, arranjo de telas, troca de tela, heartbeat, erros. Bidirecional.
    Control = 0,
    /// Teclas, botões e roda. Servidor → cliente. Perder um evento aqui é inaceitável.
    ReliableInput = 1,
    /// Movimento do ponteiro. Servidor → cliente. O mais recente vence.
    Pointer = 2,
    /// Travessia de borda de volta e avisos do cliente. Cliente → servidor.
    Feedback = 3,
    /// Oferta e transferência de texto do clipboard. Bidirecional.
    ClipboardText = 4,
    /// Imagens, arquivos e clipboard grande. Só TCP.
    Bulk = 5,
}

/// Como a aplicação trata perda neste canal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reliability {
    /// Sequência, confirmação e retransmissão. Se não der, o enlace cai.
    ///
    /// Cair é melhor que prosseguir com lacuna: um `KeyUp` perdido é uma tecla presa.
    Reliable,
    /// Sem confirmação. Mensagem com sequência anterior à última aceita é descartada.
    LatestWins,
}

/// O que fazer quando a fila deste canal enche.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Saturation {
    /// Descarta a mais antiga e segue. Só para movimento de ponteiro.
    DropOldest,
    /// Derruba o enlace. Para tudo que não pode ser perdido.
    FailLink,
}

impl ChannelId {
    /// Todos os canais, para varreduras e testes exaustivos.
    pub const ALL: [Self; 6] = [
        Self::Control,
        Self::ReliableInput,
        Self::Pointer,
        Self::Feedback,
        Self::ClipboardText,
        Self::Bulk,
    ];

    /// Converte o byte do fio em canal.
    ///
    /// # Errors
    ///
    /// [`ProtoError::UnknownChannel`] para qualquer valor fora da tabela. Não há canal
    /// "reservado para uso futuro": um byte desconhecido é um par que não entendemos, e
    /// prosseguir seria agir sob ambiguidade.
    pub const fn from_wire(byte: u8) -> Result<Self> {
        match byte {
            0 => Ok(Self::Control),
            1 => Ok(Self::ReliableInput),
            2 => Ok(Self::Pointer),
            3 => Ok(Self::Feedback),
            4 => Ok(Self::ClipboardText),
            5 => Ok(Self::Bulk),
            other => Err(ProtoError::UnknownChannel { channel: other }),
        }
    }

    /// O byte que representa este canal no fio.
    #[must_use]
    pub const fn to_wire(self) -> u8 {
        self as u8
    }

    /// Como a aplicação trata perda neste canal.
    #[must_use]
    pub const fn reliability(self) -> Reliability {
        match self {
            Self::Pointer => Reliability::LatestWins,
            Self::Control
            | Self::ReliableInput
            | Self::Feedback
            | Self::ClipboardText
            | Self::Bulk => Reliability::Reliable,
        }
    }

    /// O que fazer quando a fila enche.
    ///
    /// Deriva de [`Self::reliability`] e não é uma escolha independente: um canal confiável
    /// que descarta silenciosamente é um canal confiável quebrado.
    #[must_use]
    pub const fn saturation(self) -> Saturation {
        match self.reliability() {
            Reliability::LatestWins => Saturation::DropOldest,
            Reliability::Reliable => Saturation::FailLink,
        }
    }

    /// Se este canal pode viajar por este portador.
    ///
    /// As duas proibições duras de `docs/03-protocolo.md` §2:
    /// entrada nunca no TCP, dados nunca fora do TCP.
    #[must_use]
    pub const fn allows(self, carrier: Carrier) -> bool {
        match self {
            Self::Bulk => carrier.carries_bulk(),
            Self::ReliableInput | Self::Pointer | Self::Feedback => carrier.carries_input(),
            // Controle e texto curto de clipboard viajam por qualquer portador ativo.
            Self::Control | Self::ClipboardText => true,
        }
    }

    /// Se este canal precisa que a aplicação forneça confiabilidade sobre este portador.
    ///
    /// Sobre stream, o meio já entrega ordenado — a sequência serve só para diagnóstico.
    /// Sobre datagrama, o mecanismo de `docs/03-protocolo.md` §4.1 é obrigatório.
    #[must_use]
    pub const fn needs_app_reliability(self, carrier: Carrier) -> bool {
        matches!(self.reliability(), Reliability::Reliable)
            && matches!(carrier.delivery(), Delivery::Datagram)
    }

    /// Nome estático, para erros e logs.
    #[must_use]
    pub const fn name(self) -> ChannelName {
        ChannelName(match self {
            Self::Control => "controle",
            Self::ReliableInput => "entrada",
            Self::Pointer => "ponteiro",
            Self::Feedback => "retorno",
            Self::ClipboardText => "clipboard",
            Self::Bulk => "dados",
        })
    }
}

impl core::fmt::Display for ChannelId {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}", self.name())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wire_round_trip_for_every_channel() {
        for channel in ChannelId::ALL {
            assert_eq!(ChannelId::from_wire(channel.to_wire()), Ok(channel));
        }
    }

    #[test]
    fn unknown_wire_byte_is_refused_not_ignored() {
        for byte in 6u8..=255 {
            assert_eq!(
                ChannelId::from_wire(byte),
                Err(ProtoError::UnknownChannel { channel: byte })
            );
        }
    }

    #[test]
    fn only_pointer_tolerates_loss() {
        for channel in ChannelId::ALL {
            let expected = if channel == ChannelId::Pointer {
                Reliability::LatestWins
            } else {
                Reliability::Reliable
            };
            assert_eq!(channel.reliability(), expected);
        }
    }

    #[test]
    fn saturation_follows_reliability() {
        for channel in ChannelId::ALL {
            match channel.reliability() {
                Reliability::Reliable => assert_eq!(channel.saturation(), Saturation::FailLink),
                Reliability::LatestWins => {
                    assert_eq!(channel.saturation(), Saturation::DropOldest);
                }
            }
        }
    }

    #[test]
    fn input_channels_are_refused_on_tcp() {
        for channel in [
            ChannelId::ReliableInput,
            ChannelId::Pointer,
            ChannelId::Feedback,
        ] {
            assert!(!channel.allows(Carrier::Tcp), "{channel} não pode usar TCP");
            assert!(channel.allows(Carrier::Rfcomm));
            assert!(channel.allows(Carrier::Udp));
        }
    }

    #[test]
    fn bulk_is_refused_everywhere_but_tcp() {
        assert!(ChannelId::Bulk.allows(Carrier::Tcp));
        assert!(!ChannelId::Bulk.allows(Carrier::Rfcomm));
        assert!(!ChannelId::Bulk.allows(Carrier::Udp));
    }

    #[test]
    fn every_channel_has_at_least_one_carrier() {
        for channel in ChannelId::ALL {
            let usable = [Carrier::Rfcomm, Carrier::Udp, Carrier::Tcp]
                .into_iter()
                .any(|carrier| channel.allows(carrier));
            assert!(usable, "{channel} não pode viajar por nenhum portador");
        }
    }

    #[test]
    fn app_reliability_is_needed_only_for_reliable_channels_over_udp() {
        assert!(ChannelId::ReliableInput.needs_app_reliability(Carrier::Udp));
        assert!(!ChannelId::ReliableInput.needs_app_reliability(Carrier::Rfcomm));
        assert!(!ChannelId::Pointer.needs_app_reliability(Carrier::Udp));
    }
}
