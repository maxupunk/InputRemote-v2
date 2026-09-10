//! Amostras de ponteiro e de roda.

use serde::{Deserialize, Serialize};

use crate::ids::MonitorId;

/// Movimento relativo do ponteiro, em pixels do lado que capturou.
///
/// É `i32` e não `i16` porque amostras coalescidas somam: o canal do ponteiro descarta a
/// antiga e mantém a mais recente, mas a mais recente pode ser a soma de várias
/// (`docs/02-arquitetura.md` §6, regra 5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct PointerDelta {
    /// Deslocamento horizontal. Positivo para a direita.
    pub dx: i32,
    /// Deslocamento vertical. Positivo para baixo.
    pub dy: i32,
}

impl PointerDelta {
    /// Nenhum movimento.
    pub const ZERO: Self = Self { dx: 0, dy: 0 };

    /// Soma duas amostras, saturando em vez de estourar.
    ///
    /// Saturar é a escolha certa aqui: um estouro silencioso faria o ponteiro salvar para o
    /// canto oposto da tela, e um pânico derrubaria o serviço por causa de um movimento de
    /// mouse. Nenhum dos dois é aceitável no caminho quente.
    #[must_use]
    pub const fn coalesced_with(self, newer: Self) -> Self {
        Self {
            dx: self.dx.saturating_add(newer.dx),
            dy: self.dy.saturating_add(newer.dy),
        }
    }

    /// Se não há deslocamento nenhum.
    #[must_use]
    pub const fn is_zero(self) -> bool {
        self.dx == 0 && self.dy == 0
    }
}

/// Posição absoluta do ponteiro, normalizada dentro de um monitor.
///
/// A normalização em `0..=u16::MAX` é independente de resolução e de escala, o que faz o
/// mesmo valor significar "o mesmo ponto da tela" nas duas pontas, mesmo com monitores
/// diferentes. Os dois sistemas alvo aceitam essa forma direto: `MOUSEEVENTF_ABSOLUTE` no
/// Windows e `EV_ABS` com faixa declarada no `uinput`.
///
/// Sempre absoluta, nunca relativa, na injeção — o motivo está em
/// `docs/05-windows.md` §4.2: o sistema aplicaria a própria aceleração a um movimento
/// relativo que já vem acelerado da origem.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PointerPosition {
    /// Monitor a que a posição se refere, no arranjo anunciado.
    pub monitor: MonitorId,
    /// Posição horizontal normalizada, `0` na borda esquerda do monitor.
    pub x: u16,
    /// Posição vertical normalizada, `0` na borda superior do monitor.
    pub y: u16,
}

/// Movimento de roda, em unidades de alta resolução.
///
/// `120` é uma marcação (*notch*) — a mesma convenção de `WHEEL_DELTA` no Windows e de
/// `REL_WHEEL_HI_RES` no evdev, o que dispensa conversão nos dois backends.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct WheelDelta {
    /// Roda horizontal. Positivo para a direita.
    pub dx: i16,
    /// Roda vertical. Positivo para cima.
    pub dy: i16,
}

impl WheelDelta {
    /// Unidades por marcação de roda.
    pub const NOTCH: i16 = 120;

    /// Nenhum movimento de roda.
    pub const ZERO: Self = Self { dx: 0, dy: 0 };

    /// Uma marcação vertical, para cima.
    #[must_use]
    pub const fn up() -> Self {
        Self {
            dx: 0,
            dy: Self::NOTCH,
        }
    }

    /// Uma marcação vertical, para baixo.
    #[must_use]
    pub const fn down() -> Self {
        Self {
            dx: 0,
            dy: -Self::NOTCH,
        }
    }

    /// Soma duas amostras, saturando.
    ///
    /// A roda é coalescida junto com o movimento porque tem a mesma natureza: ninguém nota
    /// uma amostra intermediária, e todos notam atraso.
    #[must_use]
    pub const fn coalesced_with(self, newer: Self) -> Self {
        Self {
            dx: self.dx.saturating_add(newer.dx),
            dy: self.dy.saturating_add(newer.dy),
        }
    }

    /// Se não há movimento de roda.
    #[must_use]
    pub const fn is_zero(self) -> bool {
        self.dx == 0 && self.dy == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coalescing_sums_deltas() {
        let a = PointerDelta { dx: 3, dy: -4 };
        let b = PointerDelta { dx: -1, dy: 10 };
        assert_eq!(a.coalesced_with(b), PointerDelta { dx: 2, dy: 6 });
    }

    #[test]
    fn coalescing_is_zero_neutral() {
        let a = PointerDelta { dx: 7, dy: -2 };
        assert_eq!(a.coalesced_with(PointerDelta::ZERO), a);
        assert_eq!(PointerDelta::ZERO.coalesced_with(a), a);
    }

    #[test]
    fn coalescing_saturates_instead_of_overflowing() {
        let huge = PointerDelta {
            dx: i32::MAX,
            dy: i32::MIN,
        };
        let more = PointerDelta {
            dx: 1000,
            dy: -1000,
        };
        let sum = huge.coalesced_with(more);
        assert_eq!(sum.dx, i32::MAX, "não pode dar a volta");
        assert_eq!(sum.dy, i32::MIN);
    }

    #[test]
    fn wheel_notch_helpers_are_opposites() {
        assert_eq!(WheelDelta::up().dy, WheelDelta::NOTCH);
        assert_eq!(WheelDelta::down().dy, -WheelDelta::NOTCH);
        assert_eq!(
            WheelDelta::up().coalesced_with(WheelDelta::down()),
            WheelDelta::ZERO
        );
    }

    #[test]
    fn wheel_coalescing_saturates() {
        let huge = WheelDelta {
            dx: i16::MAX,
            dy: i16::MIN,
        };
        let sum = huge.coalesced_with(WheelDelta { dx: 500, dy: -500 });
        assert_eq!(sum.dx, i16::MAX);
        assert_eq!(sum.dy, i16::MIN);
    }

    #[test]
    fn zero_checks_agree_with_the_zero_constants() {
        assert!(PointerDelta::ZERO.is_zero());
        assert!(WheelDelta::ZERO.is_zero());
        assert!(!PointerDelta { dx: 0, dy: 1 }.is_zero());
        assert!(!WheelDelta { dx: 1, dy: 0 }.is_zero());
    }

    #[test]
    fn normalised_position_covers_the_whole_monitor() {
        let top_left = PointerPosition {
            monitor: MonitorId(0),
            x: 0,
            y: 0,
        };
        let bottom_right = PointerPosition {
            monitor: MonitorId(0),
            x: u16::MAX,
            y: u16::MAX,
        };
        assert_ne!(top_left, bottom_right);
        assert_eq!(top_left.monitor, bottom_right.monitor);
    }
}
