//! O backend do Linux: o portal no GNOME, o `wl-clipboard` nos outros.
//!
//! | Ambiente | Backend | Aviso de mudança |
//! |---|---|---|
//! | GNOME | [portal](portal) `org.freedesktop.portal.Clipboard`, numa sessão aprovada uma vez | `SelectionOwnerChanged` |
//! | KDE, Sway e outros com `data-control` | [`wl-clipboard`](wl) | `wl-paste --watch` |
//!
//! O GNOME não pode ficar com o `wl-clipboard`: sem `data-control`, cada `wl-copy` e `wl-paste`
//! vira um "InputRemote está pronto" e um ícone no dock ([ADR-0017](../../../../docs/adr/0017-clipboard-do-gnome-pelo-portal.md)).
//! E os outros não vão para o portal: lá o `wl-clipboard` funciona limpo, e o portal custaria um
//! diálogo de autorização sem ganho nenhum.
//!
//! Um GNOME cujo portal ainda não tem o clipboard (anterior ao 45) fica com o `wl-clipboard`: com
//! os ícones, mas copiando. `IR_CLIPBOARD=portal` ou `IR_CLIPBOARD=wl-clipboard` força a escolha,
//! para a bancada.

use std::sync::OnceLock;

use crate::error::Result;
use crate::{Clipboard, Vigia};

mod portal;
mod wl;

/// Quem atende o clipboard nesta sessão.
#[derive(Debug, Clone, Copy)]
enum Backend {
    Portal(&'static portal::Portal),
    WlClipboard,
}

/// A escolha, feita uma vez: o clipboard e o vigia têm de concordar.
fn backend() -> Backend {
    static ESCOLHA: OnceLock<Backend> = OnceLock::new();
    *ESCOLHA.get_or_init(|| {
        let forcado = std::env::var("IR_CLIPBOARD").ok();
        let quer_portal = match forcado.as_deref() {
            Some("portal") => true,
            Some("wl-clipboard") => false,
            _ => std::env::var("XDG_CURRENT_DESKTOP").is_ok_and(|ambiente| e_gnome(&ambiente)),
        };
        if !quer_portal {
            return Backend::WlClipboard;
        }
        if let Some(portal) = portal::Portal::compartilhado() {
            tracing::info!("clipboard pelo portal do GNOME");
            Backend::Portal(portal)
        } else {
            tracing::warn!(
                "o portal deste GNOME não tem o clipboard; fica o wl-clipboard, com os ícones no dock"
            );
            Backend::WlClipboard
        }
    })
}

/// Se o ambiente é o GNOME. A variável é uma lista: o Ubuntu diz `ubuntu:GNOME`.
fn e_gnome(ambiente: &str) -> bool {
    ambiente
        .split(':')
        .any(|nome| nome.eq_ignore_ascii_case("GNOME"))
}

/// Abre o clipboard desta sessão.
///
/// # Errors
///
/// [`crate::ClipError::Indisponivel`] fora de uma sessão gráfica — um *greeter*, um console, um SSH.
pub fn abrir() -> Result<Box<dyn Clipboard>> {
    match backend() {
        Backend::Portal(portal) => Ok(portal::abrir(portal)),
        Backend::WlClipboard => wl::abrir(),
    }
}

/// Começa a vigiar, se o ambiente souber avisar.
///
/// # Errors
///
/// [`crate::ClipError::Indisponivel`] onde não há aviso. Não é defeito: aí a sincronização acontece
/// na travessia ([ADR-0011](../../../../docs/adr/0011-clipboard-na-travessia.md)).
pub fn vigiar() -> Result<Box<dyn Vigia>> {
    match backend() {
        Backend::Portal(portal) => Ok(portal::vigiar(portal)),
        Backend::WlClipboard => wl::vigiar(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn o_gnome_e_reconhecido_tambem_dentro_da_lista() {
        assert!(e_gnome("GNOME"));
        assert!(e_gnome("ubuntu:GNOME"));
        assert!(e_gnome("GNOME-Classic:GNOME"));
        assert!(!e_gnome("KDE"));
        assert!(!e_gnome("sway"));
        assert!(!e_gnome(""));
    }
}
