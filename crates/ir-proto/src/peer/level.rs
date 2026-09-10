//! Os níveis de capacidade de entrada privilegiada.

use serde::{Deserialize, Serialize};

/// Até onde esta máquina consegue aceitar entrada quando não há sessão desbloqueada.
///
/// São os níveis de capacidade de `docs/01-visao-e-escopo.md` §2. Eles viajam no handshake
/// porque o **servidor** precisa saber o que o cliente consegue fazer para poder avisar
/// antes, em vez de o usuário descobrir com a tela bloqueada na frente.
///
/// A ordem da declaração é a ordem de capacidade, e `Ord` derivado respeita isso: um nível
/// maior é estritamente mais capaz.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default, Hash, Serialize, Deserialize,
)]
pub enum PrivilegedInputLevel {
    /// N0 — nenhuma entrada privilegiada. Instalação incompleta ou serviço ausente.
    #[default]
    None,
    /// N1 — só com a sessão desbloqueada. É um KVM comum; o requisito R1 não foi atendido.
    UnlockedOnly,
    /// N2 — tela de bloqueio e diálogos de elevação. É o piso aceitável do produto.
    LockScreen,
    /// N3 — tela de login, antes de qualquer usuário logado. É o alvo.
    LoginScreen,
}

impl PrivilegedInputLevel {
    /// O piso aceitável do produto (`docs/01-visao-e-escopo.md` §2).
    pub const FLOOR: Self = Self::LockScreen;

    /// Se este nível atende o requisito R1.
    #[must_use]
    pub fn meets_requirement(self) -> bool {
        self >= Self::FLOOR
    }

    /// Rótulo curto, como aparece na documentação e na interface.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::None => "N0",
            Self::UnlockedOnly => "N1",
            Self::LockScreen => "N2",
            Self::LoginScreen => "N3",
        }
    }

    /// Frase que a interface mostra ao usuário.
    #[must_use]
    pub const fn description(self) -> &'static str {
        match self {
            Self::None => "não aceita entrada privilegiada",
            Self::UnlockedOnly => "só com a sessão desbloqueada",
            Self::LockScreen => "tela de bloqueio e prompts de elevação",
            Self::LoginScreen => "tela de login, antes de qualquer usuário",
        }
    }
}

impl core::fmt::Display for PrivilegedInputLevel {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{} ({})", self.label(), self.description())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn levels_are_ordered_by_capability() {
        use PrivilegedInputLevel as L;
        assert!(L::LoginScreen > L::LockScreen);
        assert!(L::LockScreen > L::UnlockedOnly);
        assert!(L::UnlockedOnly > L::None);
    }

    #[test]
    fn the_floor_is_the_lock_screen() {
        use PrivilegedInputLevel as L;
        assert_eq!(L::FLOOR, L::LockScreen);
        assert!(L::LockScreen.meets_requirement());
        assert!(L::LoginScreen.meets_requirement());
        assert!(
            !L::UnlockedOnly.meets_requirement(),
            "N1 não atende o requisito R1"
        );
        assert!(!L::None.meets_requirement());
    }

    #[test]
    fn every_level_has_a_label_and_a_description() {
        use PrivilegedInputLevel as L;
        for level in [L::None, L::UnlockedOnly, L::LockScreen, L::LoginScreen] {
            assert!(!level.label().is_empty());
            assert!(!level.description().is_empty());
            assert!(level.to_string().contains(level.label()));
        }
    }

    #[test]
    fn default_level_is_the_most_pessimistic() {
        assert_eq!(PrivilegedInputLevel::default(), PrivilegedInputLevel::None);
    }
}
