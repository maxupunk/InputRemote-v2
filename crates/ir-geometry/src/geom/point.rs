//! Um ponto no desktop virtual.

/// Um ponto no desktop virtual, em pixels lógicos.
///
/// Coordenadas negativas são normais: o Windows põe monitores à esquerda ou acima do
/// principal em coordenadas negativas.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Point {
    /// Horizontal, positivo para a direita.
    pub x: i32,
    /// Vertical, positivo para baixo.
    pub y: i32,
}

impl Point {
    /// A origem.
    pub const ORIGIN: Self = Self { x: 0, y: 0 };

    /// Um ponto.
    #[must_use]
    pub const fn new(x: i32, y: i32) -> Self {
        Self { x, y }
    }

    /// Este ponto deslocado, saturando em vez de estourar.
    ///
    /// Saturar e não estourar: um delta absurdo vindo de um par com defeito deve grudar o
    /// ponteiro na borda, não dar a volta para o canto oposto nem derrubar o serviço.
    #[must_use]
    pub const fn offset(self, dx: i32, dy: i32) -> Self {
        Self {
            x: self.x.saturating_add(dx),
            y: self.y.saturating_add(dy),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offset_saturates_instead_of_wrapping() {
        assert_eq!(Point::new(i32::MAX, 0).offset(10, 0).x, i32::MAX);
        assert_eq!(Point::new(i32::MIN, 0).offset(-10, 0).x, i32::MIN);
    }
}
