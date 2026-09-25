//! O que uma máquina consegue fazer, do ponto de vista de quem configura.

use serde::{Deserialize, Serialize};

use ir_proto::peer::{Capabilities, PrivilegedInputLevel};

/// Até onde uma máquina consegue aceitar digitação sem sessão desbloqueada.
///
/// São os níveis N0 a N3 de [01, §2](../../../docs/01-visao-e-escopo.md). Aparecem na tela porque
/// decidem se o produto vai servir na tela de bloqueio, e descobrir que não serve *com a tela
/// bloqueada na frente* é o pior momento possível.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default, Hash, Serialize, Deserialize,
)]
pub enum Nivel {
    /// N0 — nenhuma digitação privilegiada.
    #[default]
    Nenhum,
    /// N1 — só com a sessão desbloqueada.
    SoDesbloqueado,
    /// N2 — tela de bloqueio e pedidos de permissão. É o piso do produto.
    TelaDeBloqueio,
    /// N3 — tela de login, antes de qualquer usuário.
    TelaDeLogin,
}

impl Nivel {
    /// O rótulo curto, como aparece no selo da tela.
    #[must_use]
    pub const fn rotulo(self) -> &'static str {
        match self {
            Self::Nenhum => "N0",
            Self::SoDesbloqueado => "N1",
            Self::TelaDeBloqueio => "N2",
            Self::TelaDeLogin => "N3",
        }
    }

    /// O que o nível quer dizer, em uma frase.
    #[must_use]
    pub const fn explicacao(self) -> &'static str {
        match self {
            Self::Nenhum => "não aceita digitação em tela protegida",
            Self::SoDesbloqueado => "só com a sessão desbloqueada",
            Self::TelaDeBloqueio => "tela de bloqueio e pedidos de permissão",
            Self::TelaDeLogin => "tela de login, antes de qualquer usuário",
        }
    }

    /// Se atende o requisito de digitar na tela de bloqueio.
    #[must_use]
    pub fn suficiente(self) -> bool {
        self >= Self::TelaDeBloqueio
    }

    /// O nível do protocolo correspondente — o que viaja no `Hello`.
    #[must_use]
    pub const fn no_protocolo(self) -> PrivilegedInputLevel {
        match self {
            Self::Nenhum => PrivilegedInputLevel::None,
            Self::SoDesbloqueado => PrivilegedInputLevel::UnlockedOnly,
            Self::TelaDeBloqueio => PrivilegedInputLevel::LockScreen,
            Self::TelaDeLogin => PrivilegedInputLevel::LoginScreen,
        }
    }
}

impl From<PrivilegedInputLevel> for Nivel {
    fn from(nivel: PrivilegedInputLevel) -> Self {
        match nivel {
            PrivilegedInputLevel::None => Self::Nenhum,
            PrivilegedInputLevel::UnlockedOnly => Self::SoDesbloqueado,
            PrivilegedInputLevel::LockScreen => Self::TelaDeBloqueio,
            PrivilegedInputLevel::LoginScreen => Self::TelaDeLogin,
        }
    }
}

/// O que o clipboard do par sabe tratar.
///
/// Agrupado num tipo próprio porque as três decisões andam juntas: quem pergunta "posso oferecer
/// uma imagem?" nunca precisa do resto.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Clipboard {
    /// Texto.
    pub texto: bool,
    /// Imagem.
    pub imagem: bool,
    /// Arquivos.
    pub arquivos: bool,
}

impl Clipboard {
    /// Nada suportado.
    pub const NADA: Self = Self {
        texto: false,
        imagem: false,
        arquivos: false,
    };

    /// Só texto — o que sempre funciona, em qualquer portador.
    pub const SO_TEXTO: Self = Self {
        texto: true,
        imagem: false,
        arquivos: false,
    };
}

/// O que o par declarou saber fazer, na forma em que a tela se interessa.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Recursos {
    /// O que o clipboard aceita.
    pub clipboard: Clipboard,
    /// Transferência de arquivos grandes.
    pub transferencia: bool,
    /// Até onde ele aceita digitação em tela protegida.
    pub nivel: Nivel,
    /// Se ele consegue gerar Ctrl+Alt+Del.
    pub atencao_segura: bool,
}

impl From<Capabilities> for Recursos {
    fn from(recursos: Capabilities) -> Self {
        Self {
            clipboard: Clipboard {
                texto: recursos.clipboard.text,
                imagem: recursos.clipboard.image,
                arquivos: recursos.clipboard.files,
            },
            transferencia: recursos.bulk_transfer,
            nivel: recursos.privileged_input.into(),
            atencao_segura: recursos.secure_attention,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn os_niveis_seguem_a_ordem_de_capacidade_do_protocolo() {
        use PrivilegedInputLevel as P;
        for nivel in [P::None, P::UnlockedOnly, P::LockScreen, P::LoginScreen] {
            assert_eq!(Nivel::from(nivel).suficiente(), nivel.meets_requirement());
            assert_eq!(Nivel::from(nivel).rotulo(), nivel.label());
            assert_eq!(Nivel::from(nivel).no_protocolo(), nivel);
        }
        assert!(Nivel::TelaDeLogin > Nivel::TelaDeBloqueio);
        assert!(Nivel::TelaDeBloqueio > Nivel::SoDesbloqueado);
    }

    #[test]
    fn o_piso_do_produto_e_a_tela_de_bloqueio() {
        assert!(Nivel::TelaDeBloqueio.suficiente());
        assert!(
            !Nivel::SoDesbloqueado.suficiente(),
            "N1 não atende o requisito R1"
        );
        assert!(
            !Nivel::default().suficiente(),
            "o padrão precisa ser o mais pessimista"
        );
    }
}
