//! O que cada ponta declara ser capaz de fazer.

use serde::{Deserialize, Serialize};

use super::PrivilegedInputLevel;

/// Que tipos de clipboard uma ponta sabe tratar.
///
/// Agrupado num tipo próprio, e não solto em [`Capabilities`], porque as três decisões
/// andam juntas: quem pergunta "posso oferecer uma imagem?" nunca precisa do resto.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ClipboardCapabilities {
    /// Texto UTF-8.
    pub text: bool,
    /// Imagem, canônica em PNG.
    pub image: bool,
    /// Arquivos e diretórios.
    pub files: bool,
}

impl ClipboardCapabilities {
    /// Nenhum tipo suportado.
    pub const NONE: Self = Self {
        text: false,
        image: false,
        files: false,
    };

    /// Só texto — o que sempre funciona, em qualquer portador.
    pub const TEXT_ONLY: Self = Self {
        text: true,
        image: false,
        files: false,
    };

    /// Tudo.
    pub const ALL: Self = Self {
        text: true,
        image: true,
        files: true,
    };

    /// O que as duas pontas suportam em comum.
    #[must_use]
    pub const fn intersect(self, other: Self) -> Self {
        Self {
            text: self.text && other.text,
            image: self.image && other.image,
            files: self.files && other.files,
        }
    }

    /// Se nenhum tipo é suportado.
    #[must_use]
    pub const fn is_none(self) -> bool {
        !self.text && !self.image && !self.files
    }
}

/// O que uma ponta declara ser capaz de fazer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Capabilities {
    /// Que tipos de clipboard sabe tratar.
    pub clipboard: ClipboardCapabilities,
    /// Tem canal de dados TCP disponível.
    pub bulk_transfer: bool,
    /// Até onde consegue injetar sem sessão desbloqueada.
    pub privileged_input: PrivilegedInputLevel,
    /// Consegue gerar Ctrl+Alt+Del (`SendSAS`, com a política habilitada).
    pub secure_attention: bool,
    /// Recusa ser controlada: a política dela é "só este controla o outro" (ADR-0014). Com isto,
    /// a borda do lado de lá vira parede, em vez de o ponteiro atravessar e ser devolvido.
    pub declines_control: bool,
}

impl Capabilities {
    /// O que as duas pontas conseguem fazer juntas, com `self` sendo o **cliente**.
    ///
    /// Interseção, nunca união: um recurso que só um lado suporta não existe na sessão.
    ///
    /// As duas exceções são deliberadas. O nível de entrada privilegiada e a capacidade de
    /// Ctrl+Alt+Del são as do cliente, não o mínimo dos dois — quem injeta é ele, e o
    /// servidor não tem como limitar nem ampliar isso.
    #[must_use]
    pub fn negotiated_with(self, server: Self) -> Self {
        Self {
            clipboard: self.clipboard.intersect(server.clipboard),
            bulk_transfer: self.bulk_transfer && server.bulk_transfer,
            privileged_input: self.privileged_input,
            secure_attention: self.secure_attention,
            declines_control: self.declines_control,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capabilities_intersect_never_union() {
        let client = Capabilities {
            clipboard: ClipboardCapabilities {
                text: true,
                image: true,
                files: false,
            },
            bulk_transfer: true,
            privileged_input: PrivilegedInputLevel::LockScreen,
            secure_attention: true,
            declines_control: false,
        };
        let server = Capabilities {
            clipboard: ClipboardCapabilities {
                text: true,
                image: false,
                files: true,
            },
            bulk_transfer: true,
            privileged_input: PrivilegedInputLevel::None,
            secure_attention: false,
            declines_control: false,
        };
        let joint = client.negotiated_with(server);
        assert!(joint.clipboard.text, "ambos suportam");
        assert!(!joint.clipboard.image, "só o cliente");
        assert!(!joint.clipboard.files, "só o servidor");
        assert!(joint.bulk_transfer);
        assert_eq!(
            joint.privileged_input,
            PrivilegedInputLevel::LockScreen,
            "quem injeta é o cliente; o servidor não rebaixa isso"
        );
        assert!(joint.secure_attention, "também é capacidade de quem injeta");
    }

    #[test]
    fn clipboard_intersection_is_commutative_and_idempotent() {
        let a = ClipboardCapabilities {
            text: true,
            image: true,
            files: false,
        };
        let b = ClipboardCapabilities {
            text: true,
            image: false,
            files: true,
        };
        assert_eq!(a.intersect(b), b.intersect(a));
        assert_eq!(a.intersect(a), a);
        assert_eq!(a.intersect(ClipboardCapabilities::ALL), a);
        assert_eq!(
            a.intersect(ClipboardCapabilities::NONE),
            ClipboardCapabilities::NONE
        );
    }

    #[test]
    fn clipboard_presets_are_what_they_say() {
        assert_eq!(
            ClipboardCapabilities::NONE,
            ClipboardCapabilities {
                text: false,
                image: false,
                files: false
            }
        );
        assert_eq!(
            ClipboardCapabilities::TEXT_ONLY,
            ClipboardCapabilities {
                text: true,
                image: false,
                files: false
            }
        );
        assert_eq!(
            ClipboardCapabilities::ALL,
            ClipboardCapabilities {
                text: true,
                image: true,
                files: true
            }
        );
        assert_eq!(
            ClipboardCapabilities::default(),
            ClipboardCapabilities::NONE
        );
    }

    #[test]
    fn default_capabilities_promise_nothing() {
        let empty = Capabilities::default();
        assert!(empty.clipboard.is_none());
        assert!(!empty.bulk_transfer);
        assert!(!empty.secure_attention);
        assert_eq!(empty.privileged_input, PrivilegedInputLevel::None);
    }
}
