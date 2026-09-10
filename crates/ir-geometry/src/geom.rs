//! Ponto e retângulo, em pixels lógicos do desktop virtual.
//!
//! Tudo aqui é inteiro. Não há ponto flutuante em nenhum lugar desta biblioteca, e isso é
//! deliberado: uma coordenada de ponteiro é um pixel, não uma fração dele, e `f32` não tem
//! ordenação total nem resultado exatamente reprodutível entre plataformas — o que
//! quebraria os vetores gravados e tornaria um teste de mapeamento frágil.

use ir_proto::screens::Edge;

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

/// Converte um deslocamento em pixels dentro de `span` numa fração `0..=u16::MAX`.
///
/// Arredonda para o mais próximo, e não trunca. A diferença não é cosmética: numa tela de
/// 1920 px, um pixel vale 34 unidades de fração, e truncar nas duas conversões perdia o
/// recuo de um pixel do ponto de entrada — o ponteiro voltava exatamente para a borda e
/// atravessava de novo. Foi o teste `entry_is_never_on_the_far_edge` que pegou isso.
fn normalise(offset: i64, span: u32) -> u16 {
    let last = u64::from(span).saturating_sub(1);
    if last == 0 {
        return 0;
    }
    let clamped = u64::try_from(offset.max(0)).unwrap_or(0).min(last);
    // Em u64: `last` cabe em u32 e u16::MAX é pequeno, então o produto não estoura.
    let scaled = (clamped * u64::from(u16::MAX) + last / 2) / last;
    u16::try_from(scaled).unwrap_or(u16::MAX)
}

/// Converte uma fração `0..=u16::MAX` num deslocamento em pixels dentro de `span`.
///
/// Arredonda para o mais próximo, pelo mesmo motivo de [`normalise`].
fn denormalise(fraction: u16, span: u32) -> i32 {
    let last = u64::from(span).saturating_sub(1);
    let full = u64::from(u16::MAX);
    let offset = (u64::from(fraction) * last + full / 2) / full;
    i32::try_from(offset).unwrap_or(i32::MAX)
}

/// O deslocamento da última coluna ou linha de um retângulo de largura `span`.
///
/// Saturado: uma dimensão maior que `i32::MAX` não existe em tela real, e saturar é melhor
/// que estourar num tipo que veio da rede.
fn span_as_i32(span: u32) -> i32 {
    i32::try_from(span.saturating_sub(1)).unwrap_or(i32::MAX)
}

/// A dimensão de um retângulo cujas bordas inclusivas são `low` e `high`.
fn span_from_bounds(low: i32, high: i32) -> u32 {
    let span = i64::from(high) - i64::from(low) + 1;
    u32::try_from(span.max(1)).unwrap_or(u32::MAX)
}

/// `clamp` de `i32` utilizável em contexto constante.
const fn clamp_i32(value: i32, low: i32, high: i32) -> i32 {
    if value < low {
        low
    } else if value > high {
        high
    } else {
        value
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

    #[test]
    fn normalisation_round_trips_exactly_for_every_common_resolution() {
        // Regressão do defeito que o teste `entry_is_never_on_the_far_edge` pegou: com
        // truncamento, um pixel de recuo virava zero na volta e o ponteiro quicava na borda.
        // A ida e volta é exata enquanto a tela couber em 65 536 px, o que cobre qualquer
        // resolução real com folga.
        for span in [2u32, 3, 100, 800, 1280, 1366, 1920, 2560, 3840, 7680] {
            let last = i64::from(span) - 1;
            for px in [0, 1, 2, last / 2, last - 1, last] {
                if !(0..=last).contains(&px) {
                    continue; // spans pequenos não têm todos esses pixels
                }
                let fraction = normalise(px, span);
                let back = i64::from(denormalise(fraction, span));
                assert_eq!(back, px, "span={span}, px={px}, fração={fraction}");
            }
        }
    }

    #[test]
    fn a_one_pixel_span_is_always_the_only_pixel() {
        assert_eq!(normalise(0, 1), 0);
        assert_eq!(normalise(999, 1), 0);
        assert_eq!(denormalise(u16::MAX, 1), 0);
    }

    #[test]
    fn offset_saturates_instead_of_wrapping() {
        assert_eq!(Point::new(i32::MAX, 0).offset(10, 0).x, i32::MAX);
        assert_eq!(Point::new(i32::MIN, 0).offset(-10, 0).x, i32::MIN);
    }
}
