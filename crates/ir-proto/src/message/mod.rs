//! O catálogo de mensagens, organizado por canal.
//!
//! A escolha de projeto central deste módulo: **o canal é a variante externa**. Uma
//! mensagem de bloco de arquivo não *pode* aparecer no canal do ponteiro, porque não existe
//! forma de construí-la ali. O invariante que `docs/03-protocolo.md` §4 descreve em tabela
//! passa a ser garantido pelo compilador, e não por verificação em tempo de execução.
//!
//! Isso também faz o primeiro byte do fio ser o número do canal: `postcard` codifica a
//! posição da variante, e a ordem aqui é a mesma de [`ChannelId`]. O teste
//! `variant_order_matches_channel_wire_bytes` guarda essa correspondência.

pub mod clipboard;
pub mod control;
pub mod data;
pub mod input_msg;

pub use clipboard::{ClipId, ClipKind, ClipboardMessage, DeclineReason};
pub use control::{Control, DisconnectReason, ErrorCode, Greeting, NetworkPowerSaving, PeerRole};
pub use data::{BulkMessage, CancelReason, ManifestItem, RejectReason, TransferId};
pub use input_msg::{Feedback, InputMessage, PointerMessage};

use serde::{Deserialize, Serialize};

use crate::channel::ChannelId;

/// Uma mensagem do protocolo, marcada pelo canal a que pertence.
///
/// A ordem das variantes é a ordem de [`ChannelId`] e **é o formato de fio**.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum Message {
    /// Canal 0.
    Control(Control),
    /// Canal 1.
    Input(InputMessage),
    /// Canal 2.
    Pointer(PointerMessage),
    /// Canal 3.
    Feedback(Feedback),
    /// Canal 4.
    Clipboard(ClipboardMessage),
    /// Canal 5.
    Bulk(BulkMessage),
}

impl Message {
    /// O canal desta mensagem.
    ///
    /// Não há caminho de erro: a variante *é* o canal.
    #[must_use]
    pub const fn channel(&self) -> ChannelId {
        match self {
            Self::Control(_) => ChannelId::Control,
            Self::Input(_) => ChannelId::ReliableInput,
            Self::Pointer(_) => ChannelId::Pointer,
            Self::Feedback(_) => ChannelId::Feedback,
            Self::Clipboard(_) => ChannelId::ClipboardText,
            Self::Bulk(_) => ChannelId::Bulk,
        }
    }

    /// Se esta mensagem está no caminho quente de entrada.
    ///
    /// Usado para aplicar as regras de `docs/02-arquitetura.md` §6 — sem alocação, sem log,
    /// sem espera — apenas onde elas importam.
    #[must_use]
    pub const fn is_hot_path(&self) -> bool {
        matches!(self, Self::Input(_) | Self::Pointer(_))
    }

    /// Se esta mensagem instrui a soltar tudo.
    ///
    /// Merece um método próprio porque é a mensagem que nunca pode ser perdida, atrasada nem
    /// reordenada — e quem a manipula precisa poder reconhecê-la sem casar padrão aninhado.
    #[must_use]
    pub const fn is_release_all(&self) -> bool {
        matches!(self, Self::Input(InputMessage::ReleaseAll))
    }
}

impl From<Control> for Message {
    fn from(value: Control) -> Self {
        Self::Control(value)
    }
}

impl From<InputMessage> for Message {
    fn from(value: InputMessage) -> Self {
        Self::Input(value)
    }
}

impl From<PointerMessage> for Message {
    fn from(value: PointerMessage) -> Self {
        Self::Pointer(value)
    }
}

impl From<Feedback> for Message {
    fn from(value: Feedback) -> Self {
        Self::Feedback(value)
    }
}

impl From<ClipboardMessage> for Message {
    fn from(value: ClipboardMessage) -> Self {
        Self::Clipboard(value)
    }
}

impl From<BulkMessage> for Message {
    fn from(value: BulkMessage) -> Self {
        Self::Bulk(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::{HidUsage, Modifiers, PointerDelta};

    fn one_of_each() -> Vec<Message> {
        vec![
            Message::Control(Control::AckOnly),
            Message::Input(InputMessage::ReleaseAll),
            Message::Pointer(PointerMessage::Motion {
                delta: PointerDelta::ZERO,
                mods: Modifiers::NONE,
            }),
            Message::Feedback(Feedback::EmergencyRelease),
            Message::Clipboard(ClipboardMessage::Request { id: ClipId(1) }),
            Message::Bulk(BulkMessage::Accept { id: TransferId(1) }),
        ]
    }

    #[test]
    fn every_channel_has_exactly_one_message_variant() {
        let messages = one_of_each();
        assert_eq!(messages.len(), ChannelId::ALL.len());
        for (message, channel) in messages.iter().zip(ChannelId::ALL) {
            assert_eq!(message.channel(), channel);
        }
    }

    #[test]
    fn variant_order_matches_channel_wire_bytes() {
        // O primeiro byte codificado é o índice da variante em postcard, e ele precisa ser
        // igual ao byte de canal da especificação. Se alguém reordenar as variantes, este
        // teste falha — que é exatamente o ponto, porque a quebra seria silenciosa.
        for message in one_of_each() {
            let bytes = postcard::to_allocvec(&message).unwrap();
            assert_eq!(
                bytes.first().copied(),
                Some(message.channel().to_wire()),
                "primeiro byte de {message:?} deveria ser o canal"
            );
        }
    }

    #[test]
    fn only_input_and_pointer_are_hot_path() {
        for message in one_of_each() {
            let expected = matches!(
                message.channel(),
                ChannelId::ReliableInput | ChannelId::Pointer
            );
            assert_eq!(message.is_hot_path(), expected, "{message:?}");
        }
    }

    #[test]
    fn release_all_is_recognised_and_nothing_else_is() {
        assert!(Message::Input(InputMessage::ReleaseAll).is_release_all());
        let key = Message::Input(InputMessage::KeyUp {
            usage: HidUsage(0x04),
            mods: Modifiers::NONE,
        });
        assert!(!key.is_release_all());
        assert!(!Message::Control(Control::AckOnly).is_release_all());
    }

    #[test]
    fn conversions_land_on_the_right_channel() {
        assert_eq!(
            Message::from(Control::AckOnly).channel(),
            ChannelId::Control
        );
        assert_eq!(
            Message::from(InputMessage::ReleaseAll).channel(),
            ChannelId::ReliableInput
        );
        assert_eq!(
            Message::from(Feedback::EmergencyRelease).channel(),
            ChannelId::Feedback
        );
        assert_eq!(
            Message::from(BulkMessage::Accept { id: TransferId(0) }).channel(),
            ChannelId::Bulk
        );
    }

    #[test]
    fn every_message_channel_allows_at_least_one_carrier() {
        for message in one_of_each() {
            let channel = message.channel();
            let any = [
                crate::Carrier::Rfcomm,
                crate::Carrier::Udp,
                crate::Carrier::Tcp,
            ]
            .into_iter()
            .any(|carrier| channel.allows(carrier));
            assert!(any, "{message:?} não tem portador possível");
        }
    }
}
