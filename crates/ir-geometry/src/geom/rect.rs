//! Um retângulo em pixels lógicos, com bordas inclusivas.

use ir_proto::screens::Edge;

use super::point::Point;
use super::scale::{clamp_i32, denormalise, normalise, span_as_i32, span_from_bounds};

/// Um retângulo, em pixels lógicos.
///
/// A largura e a altura são positivas por construção. Um retângulo de área zero não existe
/// neste tipo — [`Rect::new`] devolve `None` para ele —, o que elimina divisão por zero de
/// todo o resto da biblioteca.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Rect {
    x: i32,
    y: i32,
    width: u32,
    height: u32,
}

impl Rect {
    /// Constrói, recusando largura ou altura zero.
    #[must_use]
    pub const fn new(x: i32, y: i32, width: u32, height: u32) -> Option<Self> {
        if width == 0 || height == 0 {
            None
        } else {
            Some(Self {
                x,
                y,
                width,
                height,
            })
        }
    }

    /// Borda esquerda, inclusiva.
    #[must_use]
    pub const fn left(self) -> i32 {
        self.x
    }

    /// Borda superior, inclusiva.
    #[must_use]
    pub const fn top(self) -> i32 {
        self.y
    }

    /// Largura, sempre positiva.
    #[must_use]
    pub const fn width(self) -> u32 {
        self.width
    }

    /// Altura, sempre positiva.
    #[must_use]
    pub const fn height(self) -> u32 {
        self.height
    }

    /// Borda direita, **inclusiva** — a coluna do último pixel.
    ///
    /// Inclusiva e não exclusiva porque a pergunta que este tipo responde é sempre "o
    /// ponteiro está no último pixel?", e um limite exclusivo convida ao erro de um.
    #[must_use]
    pub fn right(self) -> i32 {
        self.x.saturating_add(span_as_i32(self.width))
    }

    /// Borda inferior, inclusiva.
    #[must_use]
    pub fn bottom(self) -> i32 {
        self.y.saturating_add(span_as_i32(self.height))
    }

    /// A coordenada da borda dada.
    #[must_use]
    pub fn edge(self, edge: Edge) -> i32 {
        match edge {
            Edge::Left => self.left(),
            Edge::Right => self.right(),
            Edge::Top => self.top(),
            Edge::Bottom => self.bottom(),
        }
    }

    /// Se o ponto está dentro, bordas inclusive.
    #[must_use]
    pub fn contains(self, point: Point) -> bool {
        point.x >= self.left()
            && point.x <= self.right()
            && point.y >= self.top()
            && point.y <= self.bottom()
    }

    /// O ponto mais próximo que está dentro deste retângulo.
    ///
    /// Existe para que nenhuma coordenada inválida chegue ao injetor. Um monitor removido
    /// no meio da sessão deixa a posição corrente fora de qualquer tela, e o certo é grudar
    /// na borda mais próxima — não recusar, não entrar em pânico.
    #[must_use]
    pub fn clamp(self, point: Point) -> Point {
        // Escrito à mão porque `Ord::clamp` ainda não é `const`, e esta função precisa ser
        // utilizável em contexto constante como o resto do tipo.
        Point {
            x: clamp_i32(point.x, self.left(), self.right()),
            y: clamp_i32(point.y, self.top(), self.bottom()),
        }
    }

    /// O menor retângulo que contém os dois.
    #[must_use]
    pub fn union(self, other: Self) -> Self {
        let left = if self.left() < other.left() {
            self.left()
        } else {
            other.left()
        };
        let top = if self.top() < other.top() {
            self.top()
        } else {
            other.top()
        };
        let right = if self.right() > other.right() {
            self.right()
        } else {
            other.right()
        };
        let bottom = if self.bottom() > other.bottom() {
            self.bottom()
        } else {
            other.bottom()
        };
        // `right` e `bottom` são inclusivos, então a dimensão é a diferença mais um. A
        // subtração é feita em i64 para não estourar com origens nos extremos de i32.
        Self {
            x: left,
            y: top,
            width: span_from_bounds(left, right),
            height: span_from_bounds(top, bottom),
        }
    }

    /// Onde o ponto está ao longo da borda dada, normalizado em `0..=u16::MAX`.
    ///
    /// Para borda horizontal (`Left`, `Right`), mede a posição vertical; para borda
    /// vertical, a horizontal. É a fração que atravessa a rede na travessia de tela: ela é
    /// independente de resolução, então "no meio da borda" continua sendo no meio do outro
    /// lado, mesmo com monitores de tamanhos diferentes.
    #[must_use]
    pub fn fraction_along(self, edge: Edge, point: Point) -> u16 {
        if edge.is_horizontal() {
            self.fraction_y(point)
        } else {
            self.fraction_x(point)
        }
    }

    /// A posição horizontal do ponto dentro deste retângulo, em `0..=u16::MAX`.
    #[must_use]
    pub fn fraction_x(self, point: Point) -> u16 {
        normalise(i64::from(point.x) - i64::from(self.left()), self.width)
    }

    /// A posição vertical do ponto dentro deste retângulo, em `0..=u16::MAX`.
    #[must_use]
    pub fn fraction_y(self, point: Point) -> u16 {
        normalise(i64::from(point.y) - i64::from(self.top()), self.height)
    }

    /// A coordenada horizontal correspondente à fração. **Sem recuo de borda.**
    ///
    /// Separada de [`Rect::point_along`] de propósito: converter posição do protocolo em
    /// ponto tem de ser exato, enquanto entrar por uma borda precisa do recuo de um pixel.
    /// Usar a mesma função para as duas coisas deslocava o ponteiro um pixel a cada
    /// conversão — defeito que o teste `entry_is_never_on_the_far_edge` pegou.
    #[must_use]
    pub fn x_at(self, fraction: u16) -> i32 {
        self.left()
            .saturating_add(denormalise(fraction, self.width))
    }

    /// A coordenada vertical correspondente à fração. Sem recuo de borda.
    #[must_use]
    pub fn y_at(self, fraction: u16) -> i32 {
        self.top()
            .saturating_add(denormalise(fraction, self.height))
    }

    /// O ponto na borda dada correspondente à fração, deslocado para **dentro**.
    ///
    /// O deslocamento de um pixel para dentro é o que impede o ponteiro de nascer
    /// exatamente na borda e disparar imediatamente uma nova travessia de volta — o
    /// ping-pong clássico desta classe de software.
    #[must_use]
    pub fn point_along(self, edge: Edge, fraction: u16) -> Point {
        let inset = |value: i32, span: u32, toward_start: bool| {
            if span <= 2 {
                value
            } else if toward_start {
                value.saturating_add(1)
            } else {
                value.saturating_sub(1)
            }
        };

        if edge.is_horizontal() {
            let x = match edge {
                Edge::Left => inset(self.left(), self.width, true),
                _ => inset(self.right(), self.width, false),
            };
            Point {
                x,
                y: self.y_at(fraction),
            }
        } else {
            let y = match edge {
                Edge::Top => inset(self.top(), self.height, true),
                _ => inset(self.bottom(), self.height, false),
            };
            Point {
                x: self.x_at(fraction),
                y,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(x: i32, y: i32, w: u32, h: u32) -> Rect {
        Rect::new(x, y, w, h).expect("retângulo válido")
    }

    #[test]
    fn zero_sized_rects_cannot_exist() {
        assert!(Rect::new(0, 0, 0, 100).is_none());
        assert!(Rect::new(0, 0, 100, 0).is_none());
        assert!(Rect::new(0, 0, 1, 1).is_some());
    }

    #[test]
    fn edges_are_inclusive() {
        let r = rect(0, 0, 1920, 1080);
        assert_eq!(r.left(), 0);
        assert_eq!(r.top(), 0);
        assert_eq!(r.right(), 1919, "a última coluna, não a largura");
        assert_eq!(r.bottom(), 1079);
    }

    #[test]
    fn contains_includes_all_four_borders() {
        let r = rect(0, 0, 100, 100);
        for point in [
            Point::new(0, 0),
            Point::new(99, 0),
            Point::new(0, 99),
            Point::new(99, 99),
        ] {
            assert!(
                r.contains(point),
                "{point:?} está na borda e deveria contar"
            );
        }
        assert!(!r.contains(Point::new(100, 50)));
        assert!(!r.contains(Point::new(-1, 50)));
    }

    #[test]
    fn clamp_sticks_to_the_nearest_border() {
        let r = rect(0, 0, 100, 100);
        assert_eq!(r.clamp(Point::new(-50, 50)), Point::new(0, 50));
        assert_eq!(r.clamp(Point::new(500, 50)), Point::new(99, 50));
        assert_eq!(r.clamp(Point::new(50, -50)), Point::new(50, 0));
        assert_eq!(r.clamp(Point::new(500, 500)), Point::new(99, 99));
        assert_eq!(
            r.clamp(Point::new(50, 50)),
            Point::new(50, 50),
            "dentro não muda"
        );
    }

    #[test]
    fn union_covers_both_including_negative_origins() {
        let a = rect(0, 0, 1920, 1080);
        let b = rect(-1920, 0, 1920, 1080);
        let u = a.union(b);
        assert_eq!(u.left(), -1920);
        assert_eq!(u.right(), 1919);
        assert_eq!(u.width(), 3840);
        assert_eq!(u.height(), 1080);
    }

    #[test]
    fn union_with_self_is_self() {
        let a = rect(-100, -200, 300, 400);
        assert_eq!(a.union(a), a);
    }

    #[test]
    fn fraction_is_zero_at_the_start_and_max_at_the_end() {
        let r = rect(0, 0, 1920, 1080);
        assert_eq!(r.fraction_along(Edge::Right, Point::new(1919, 0)), 0);
        assert_eq!(
            r.fraction_along(Edge::Right, Point::new(1919, 1079)),
            u16::MAX
        );
        assert_eq!(r.fraction_along(Edge::Bottom, Point::new(0, 1079)), 0);
        assert_eq!(
            r.fraction_along(Edge::Bottom, Point::new(1919, 1079)),
            u16::MAX
        );
    }

    #[test]
    fn fraction_is_about_half_in_the_middle() {
        let r = rect(0, 0, 1920, 1080);
        let half = r.fraction_along(Edge::Right, Point::new(1919, 539));
        let expected = u32::from(u16::MAX) / 2;
        let got = u32::from(half);
        assert!(
            got.abs_diff(expected) < 100,
            "{got} deveria estar perto de {expected}"
        );
    }

    #[test]
    fn fraction_ignores_the_offset_of_the_desktop() {
        // Um desktop com origem negativa produz a mesma fração de um na origem.
        let at_origin = rect(0, 0, 1000, 1000);
        let shifted = rect(-500, -700, 1000, 1000);
        assert_eq!(
            at_origin.fraction_along(Edge::Right, Point::new(999, 250)),
            shifted.fraction_along(Edge::Right, Point::new(499, -450))
        );
    }

    #[test]
    fn fraction_clamps_points_outside_the_rect() {
        let r = rect(0, 0, 100, 100);
        assert_eq!(r.fraction_along(Edge::Right, Point::new(0, -500)), 0);
        assert_eq!(r.fraction_along(Edge::Right, Point::new(0, 500)), u16::MAX);
    }

    #[test]
    fn point_along_lands_inside_the_rect() {
        let r = rect(0, 0, 1920, 1080);
        for edge in Edge::ALL {
            for fraction in [0u16, 1, 30_000, u16::MAX / 2, u16::MAX - 1, u16::MAX] {
                let point = r.point_along(edge, fraction);
                assert!(
                    r.contains(point),
                    "{edge} com fração {fraction} caiu fora: {point:?}"
                );
            }
        }
    }

    #[test]
    fn point_along_is_inset_one_pixel_from_the_edge() {
        // O recuo é o que impede o ponteiro de nascer na borda e disparar a travessia de
        // volta imediatamente.
        let r = rect(0, 0, 1920, 1080);
        assert_eq!(r.point_along(Edge::Left, 0).x, 1);
        assert_eq!(r.point_along(Edge::Right, 0).x, 1918);
        assert_eq!(r.point_along(Edge::Top, 0).y, 1);
        assert_eq!(r.point_along(Edge::Bottom, 0).y, 1078);
    }

    #[test]
    fn a_one_pixel_rect_never_produces_an_invalid_point() {
        let tiny = rect(7, 9, 1, 1);
        for edge in Edge::ALL {
            for fraction in [0u16, u16::MAX] {
                let point = tiny.point_along(edge, fraction);
                assert_eq!(point, Point::new(7, 9));
                assert!(tiny.contains(point));
            }
        }
        assert_eq!(tiny.fraction_along(Edge::Right, Point::new(7, 9)), 0);
    }

    #[test]
    fn fraction_and_point_along_are_inverse_within_a_pixel() {
        let r = rect(-1920, 0, 1920, 1080);
        for y in [0i32, 1, 270, 539, 540, 1078, 1079] {
            let fraction = r.fraction_along(Edge::Left, Point::new(-1920, y));
            let back = r.point_along(Edge::Left, fraction);
            assert!(back.y.abs_diff(y) <= 1, "y={y} voltou como {}", back.y);
        }
    }
}
