//! Arranjo de telas e bordas, na forma que viaja no fio.
//!
//! Estes tipos são deliberadamente pobres: retângulos, escala e uma lista. Toda a
//! inteligência de mapear coordenada entre arranjos diferentes vive em `ir-geometry`, que
//! consome estes tipos. A separação existe porque `ir-proto` não pode depender de mais
//! nada (`docs/02-arquitetura.md` §2), e porque o formato de fio precisa ser estável
//! enquanto o algoritmo de mapeamento pode evoluir.

use serde::{Deserialize, Serialize};

use crate::error::{ProtoError, Result};
use crate::ids::MonitorId;
use crate::limits;

/// Uma das quatro bordas de uma tela.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Edge {
    /// Borda esquerda.
    Left,
    /// Borda direita.
    Right,
    /// Borda superior.
    Top,
    /// Borda inferior.
    Bottom,
}

impl Edge {
    /// Todas as bordas.
    pub const ALL: [Self; 4] = [Self::Left, Self::Right, Self::Top, Self::Bottom];

    /// A borda por onde se entra quando se sai por esta.
    ///
    /// Sair pela direita de uma tela é entrar pela esquerda da outra. Essa simetria é o que
    /// faz a travessia parecer um monitor a mais, e é usada nas duas direções.
    #[must_use]
    pub const fn opposite(self) -> Self {
        match self {
            Self::Left => Self::Right,
            Self::Right => Self::Left,
            Self::Top => Self::Bottom,
            Self::Bottom => Self::Top,
        }
    }

    /// Se a travessia por esta borda é horizontal.
    #[must_use]
    pub const fn is_horizontal(self) -> bool {
        matches!(self, Self::Left | Self::Right)
    }

    /// Nome estável, para interface e log.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Left => "esquerda",
            Self::Right => "direita",
            Self::Top => "acima",
            Self::Bottom => "abaixo",
        }
    }
}

impl core::fmt::Display for Edge {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.name())
    }
}

/// Um monitor, como a outra ponta o descreve.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct MonitorInfo {
    /// Identificador dentro deste arranjo.
    pub id: MonitorId,
    /// Origem horizontal no desktop virtual, em pixels lógicos. Pode ser negativa.
    pub x: i32,
    /// Origem vertical no desktop virtual, em pixels lógicos. Pode ser negativa.
    pub y: i32,
    /// Largura em pixels lógicos. Zero é inválido.
    pub width: u32,
    /// Altura em pixels lógicos. Zero é inválido.
    pub height: u32,
    /// Escala em milésimos: `1000` é 100%, `1500` é 150%.
    ///
    /// Milésimos e não ponto flutuante porque o formato de fio precisa ser exatamente
    /// reprodutível para os vetores gravados, e porque `f32` não tem ordenação total.
    pub scale_permille: u16,
    /// Se é o monitor principal do arranjo.
    pub primary: bool,
}

impl MonitorInfo {
    /// Se as dimensões fazem sentido.
    ///
    /// Um monitor de largura zero viria de um par com defeito ou de um par malicioso, e
    /// dividir por ela ao mapear coordenada seria pânico num processo privilegiado.
    #[must_use]
    pub const fn is_valid(&self) -> bool {
        self.width > 0 && self.height > 0 && self.scale_permille > 0
    }
}

/// O arranjo completo de telas de uma ponta.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(try_from = "Vec<MonitorInfo>", into = "Vec<MonitorInfo>")]
pub struct ScreenLayout {
    monitors: Vec<MonitorInfo>,
}

impl ScreenLayout {
    /// Constrói validando contagem, dimensões e unicidade de identificador.
    ///
    /// # Errors
    ///
    /// - [`ProtoError::CountTooLarge`] acima de [`limits::MAX_MONITORS`]. Verificado
    ///   **antes** de qualquer outra coisa, porque a contagem vem do fio.
    /// - [`ProtoError::Malformed`] para dimensão zero, escala zero ou identificador
    ///   repetido.
    pub fn new(monitors: Vec<MonitorInfo>) -> Result<Self> {
        if monitors.len() > limits::MAX_MONITORS {
            return Err(ProtoError::CountTooLarge {
                what: "monitores",
                actual: monitors.len(),
                limit: limits::MAX_MONITORS,
            });
        }
        if monitors.iter().any(|m| !m.is_valid()) {
            return Err(ProtoError::Malformed);
        }
        for (index, monitor) in monitors.iter().enumerate() {
            if monitors
                .iter()
                .skip(index + 1)
                .any(|other| other.id == monitor.id)
            {
                return Err(ProtoError::Malformed);
            }
        }
        Ok(Self { monitors })
    }

    /// Um arranjo de uma tela só, o caso mais comum.
    ///
    /// # Errors
    ///
    /// [`ProtoError::Malformed`] se as dimensões não fizerem sentido.
    pub fn single(width: u32, height: u32) -> Result<Self> {
        Self::new(vec![MonitorInfo {
            id: MonitorId(0),
            x: 0,
            y: 0,
            width,
            height,
            scale_permille: 1000,
            primary: true,
        }])
    }

    /// Os monitores do arranjo.
    #[must_use]
    pub fn monitors(&self) -> &[MonitorInfo] {
        &self.monitors
    }

    /// Quantos monitores há.
    #[must_use]
    pub fn len(&self) -> usize {
        self.monitors.len()
    }

    /// Se o arranjo está vazio.
    ///
    /// Acontece de verdade: uma máquina sem tela conectada, ou durante uma troca de
    /// monitor. Não é erro, e o produto precisa lidar sem coordenada inválida.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.monitors.is_empty()
    }

    /// O monitor com este identificador.
    #[must_use]
    pub fn get(&self, id: MonitorId) -> Option<&MonitorInfo> {
        self.monitors.iter().find(|m| m.id == id)
    }

    /// O monitor principal, ou o primeiro, ou nenhum se o arranjo estiver vazio.
    #[must_use]
    pub fn primary(&self) -> Option<&MonitorInfo> {
        self.monitors
            .iter()
            .find(|m| m.primary)
            .or_else(|| self.monitors.first())
    }
}

impl TryFrom<Vec<MonitorInfo>> for ScreenLayout {
    type Error = ProtoError;

    fn try_from(monitors: Vec<MonitorInfo>) -> Result<Self> {
        Self::new(monitors)
    }
}

impl From<ScreenLayout> for Vec<MonitorInfo> {
    fn from(layout: ScreenLayout) -> Self {
        layout.monitors
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn monitor(id: u8, x: i32, width: u32) -> MonitorInfo {
        MonitorInfo {
            id: MonitorId(id),
            x,
            y: 0,
            width,
            height: 1080,
            scale_permille: 1000,
            primary: id == 0,
        }
    }

    #[test]
    fn opposite_edges_round_trip() {
        for edge in Edge::ALL {
            assert_eq!(edge.opposite().opposite(), edge);
            assert_ne!(edge.opposite(), edge);
        }
    }

    #[test]
    fn opposite_preserves_orientation() {
        for edge in Edge::ALL {
            assert_eq!(edge.is_horizontal(), edge.opposite().is_horizontal());
        }
    }

    #[test]
    fn a_single_screen_layout_is_primary_and_valid() {
        let layout = ScreenLayout::single(1920, 1080).unwrap();
        assert_eq!(layout.len(), 1);
        assert!(layout.primary().is_some());
        assert!(layout.get(MonitorId(0)).is_some());
    }

    #[test]
    fn zero_sized_monitors_are_refused() {
        assert_eq!(
            ScreenLayout::single(0, 1080).unwrap_err(),
            ProtoError::Malformed
        );
        assert_eq!(
            ScreenLayout::single(1920, 0).unwrap_err(),
            ProtoError::Malformed
        );
    }

    #[test]
    fn zero_scale_is_refused() {
        let mut bad = monitor(0, 0, 1920);
        bad.scale_permille = 0;
        assert_eq!(
            ScreenLayout::new(vec![bad]).unwrap_err(),
            ProtoError::Malformed
        );
    }

    #[test]
    fn duplicate_monitor_ids_are_refused() {
        let layout = ScreenLayout::new(vec![monitor(0, 0, 1920), monitor(0, 1920, 1920)]);
        assert_eq!(layout.unwrap_err(), ProtoError::Malformed);
    }

    #[test]
    fn count_is_checked_before_anything_else() {
        // Todos inválidos E acima do limite: o erro tem de ser o de contagem, provando que
        // a verificação barata acontece antes da varredura.
        let flood = vec![
            MonitorInfo {
                id: MonitorId(0),
                x: 0,
                y: 0,
                width: 0,
                height: 0,
                scale_permille: 0,
                primary: false,
            };
            limits::MAX_MONITORS + 1
        ];
        let err = ScreenLayout::new(flood).unwrap_err();
        assert!(matches!(
            err,
            ProtoError::CountTooLarge {
                what: "monitores",
                ..
            }
        ));
    }

    #[test]
    fn an_empty_layout_is_allowed_and_reports_no_primary() {
        let layout = ScreenLayout::new(Vec::new()).unwrap();
        assert!(layout.is_empty());
        assert!(layout.primary().is_none());
        assert!(layout.get(MonitorId(0)).is_none());
    }

    #[test]
    fn primary_falls_back_to_the_first_monitor() {
        let mut first = monitor(1, 0, 1920);
        first.primary = false;
        let second = {
            let mut m = monitor(2, 1920, 1920);
            m.primary = false;
            m
        };
        let layout = ScreenLayout::new(vec![first, second]).unwrap();
        assert_eq!(layout.primary().map(|m| m.id), Some(MonitorId(1)));
    }

    #[test]
    fn negative_origins_are_accepted() {
        // Windows põe monitores à esquerda do principal em coordenadas negativas.
        let layout = ScreenLayout::new(vec![monitor(0, 0, 1920), monitor(1, -1920, 1920)]);
        assert_eq!(layout.unwrap().len(), 2);
    }
}
