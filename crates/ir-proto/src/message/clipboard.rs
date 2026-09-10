//! Canal 4 — texto do clipboard.
//!
//! Só texto, e só até [`limits::MAX_CLIPBOARD_TEXT_OFF_TCP`]. Imagem, arquivo e texto
//! grande vão pelo canal 5, que existe apenas sobre TCP — o rádio Bluetooth não disputa com
//! o ponteiro (`docs/03-protocolo.md` §2).

use serde::{Deserialize, Serialize};

use crate::error::{ProtoError, Result};
use crate::limits;

/// Que tipo de conteúdo há no clipboard.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum ClipKind {
    /// Texto UTF-8. No protocolo, sempre com LF; a conversão para CRLF é do backend Windows.
    Text,
    /// Imagem. Canônica em PNG, qualquer que seja o formato nativo.
    Image,
    /// Lista de arquivos e diretórios.
    Files,
}

impl ClipKind {
    /// Se este tipo pode viajar pelo canal 4.
    ///
    /// Só texto. É a regra que impede uma imagem de 8 MB entrar no rádio Bluetooth.
    #[must_use]
    pub const fn fits_text_channel(self) -> bool {
        matches!(self, Self::Text)
    }

    /// Nome estável, para interface e diagnóstico.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Text => "texto",
            Self::Image => "imagem",
            Self::Files => "arquivos",
        }
    }
}

/// Identificador de uma oferta de clipboard.
///
/// Serve para casar oferta, pedido e pedaços, e para descartar pedaços de uma oferta que já
/// foi substituída por outra mais nova — o que acontece toda vez que o usuário copia duas
/// coisas em sequência rápida.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ClipId(pub u32);

/// Mensagem do canal de texto do clipboard.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum ClipboardMessage {
    /// "Tenho isto para oferecer."
    ///
    /// A oferta vem antes do conteúdo para que o outro lado possa recusar sem tráfego —
    /// por exemplo quando o conteúdo é grande demais para o portador atual.
    Offer {
        /// Identificador desta oferta.
        id: ClipId,
        /// Que tipo de conteúdo.
        kind: ClipKind,
        /// Tamanho total em bytes.
        size: u32,
        /// BLAKE3 do conteúdo, para o destino conferir.
        hash: [u8; 32],
    },
    /// "Manda."
    Request {
        /// A oferta desejada.
        id: ClipId,
    },
    /// Um pedaço do conteúdo.
    Chunk {
        /// A oferta a que este pedaço pertence.
        id: ClipId,
        /// Índice do pedaço, começando em zero.
        index: u32,
        /// Os bytes.
        data: Vec<u8>,
    },
    /// Fim do conteúdo.
    Done {
        /// A oferta concluída.
        id: ClipId,
    },
    /// "Não vou pegar", com motivo.
    Decline {
        /// A oferta recusada.
        id: ClipId,
        /// Por quê.
        reason: DeclineReason,
    },
}

/// Por que uma oferta de clipboard foi recusada.
///
/// Existe para a interface poder dizer o motivo. No v1, conteúdo simplesmente não aparecia
/// do outro lado e não havia como saber por quê.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum DeclineReason {
    /// Passa do limite deste canal e não há portador de dados disponível.
    TooLargeForCarrier,
    /// Este tipo não está habilitado nas capacidades negociadas.
    KindNotSupported,
    /// Já há uma oferta mais nova; esta perdeu a validade.
    Superseded,
    /// O usuário desligou a sincronização de clipboard.
    Disabled,
}

impl DeclineReason {
    /// Frase para a interface.
    #[must_use]
    pub const fn description(self) -> &'static str {
        match self {
            Self::TooLargeForCarrier => "grande demais para a conexão atual",
            Self::KindNotSupported => "tipo não suportado pelo outro computador",
            Self::Superseded => "substituída por uma cópia mais nova",
            Self::Disabled => "sincronização de clipboard desligada",
        }
    }
}

/// Decide se um conteúdo pode ir pelo canal 4 ou precisa do canal 5.
///
/// Concentrar a decisão aqui evita que ela seja tomada de forma diferente em dois lugares —
/// que foi como o v1 acabou com regras de degradação divergentes por modo
/// (`docs/00-licoes-do-v1.md` §6).
///
/// # Errors
///
/// [`ProtoError::TooLarge`] quando nem o canal 5 resolveria, isto é, quando não há portador
/// de dados. O chamador transforma isso numa [`DeclineReason::TooLargeForCarrier`].
pub fn route(kind: ClipKind, size: u32, bulk_available: bool) -> Result<Route> {
    let size = usize::try_from(size).unwrap_or(usize::MAX);

    if kind.fits_text_channel() && size <= limits::MAX_CLIPBOARD_TEXT_OFF_TCP {
        return Ok(Route::TextChannel);
    }
    if bulk_available {
        return Ok(Route::BulkChannel);
    }
    Err(ProtoError::TooLarge {
        actual: size,
        limit: limits::MAX_CLIPBOARD_TEXT_OFF_TCP,
    })
}

/// Por onde um conteúdo de clipboard deve viajar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Route {
    /// Canal 4, em qualquer portador ativo.
    TextChannel,
    /// Canal 5, só sobre TCP.
    BulkChannel,
}

#[cfg(test)]
mod tests {
    use super::*;

    const SMALL: u32 = 128;

    /// O limite do canal 4, como `u32`, sem cast que possa truncar.
    fn text_limit() -> u32 {
        u32::try_from(limits::MAX_CLIPBOARD_TEXT_OFF_TCP).expect("limite cabe em u32")
    }

    /// Um byte acima do limite do canal 4.
    fn over_limit() -> u32 {
        text_limit() + 1
    }

    #[test]
    fn only_text_fits_the_text_channel() {
        assert!(ClipKind::Text.fits_text_channel());
        assert!(!ClipKind::Image.fits_text_channel());
        assert!(!ClipKind::Files.fits_text_channel());
    }

    #[test]
    fn small_text_goes_through_the_text_channel_even_without_tcp() {
        assert_eq!(
            route(ClipKind::Text, SMALL, false).unwrap(),
            Route::TextChannel
        );
    }

    #[test]
    fn text_at_exactly_the_limit_still_uses_the_text_channel() {
        assert_eq!(
            route(ClipKind::Text, text_limit(), false).unwrap(),
            Route::TextChannel
        );
    }

    #[test]
    fn big_text_needs_tcp() {
        assert_eq!(
            route(ClipKind::Text, over_limit(), true).unwrap(),
            Route::BulkChannel
        );
        let err = route(ClipKind::Text, over_limit(), false).unwrap_err();
        assert!(matches!(err, ProtoError::TooLarge { .. }));
    }

    #[test]
    fn images_and_files_always_need_tcp_however_small() {
        for kind in [ClipKind::Image, ClipKind::Files] {
            assert_eq!(
                route(kind, 1, true).unwrap(),
                Route::BulkChannel,
                "{kind:?}"
            );
            assert!(
                route(kind, 1, false).is_err(),
                "{kind:?} sem TCP não tem caminho"
            );
        }
    }

    #[test]
    fn every_decline_reason_has_a_human_description() {
        use DeclineReason as D;
        for reason in [
            D::TooLargeForCarrier,
            D::KindNotSupported,
            D::Superseded,
            D::Disabled,
        ] {
            assert!(!reason.description().is_empty(), "{reason:?}");
        }
    }

    #[test]
    fn every_kind_has_a_name() {
        for kind in [ClipKind::Text, ClipKind::Image, ClipKind::Files] {
            assert!(!kind.name().is_empty());
        }
    }
}
