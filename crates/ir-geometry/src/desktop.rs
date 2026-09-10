//! O desktop virtual: os monitores de uma ponta, e o que se pergunta a eles.
//!
//! Um `Desktop` é construído a partir do [`ScreenLayout`] que a outra ponta anunciou, e é
//! sempre válido depois de construído: todos os monitores têm área positiva, os
//! identificadores não repetem, e a união deles é um retângulo utilizável. Toda a validação
//! acontece na construção, uma vez, e não em cada consulta.
//!
//! A distinção que importa: [`Desktop::bounds`] é o retângulo que **envolve** os monitores,
//! e pode conter buracos. Num arranjo em L, o canto vazio está dentro de `bounds` e fora de
//! qualquer monitor. Por isso [`Desktop::monitor_at`] devolve `Option`, e por isso existe
//! [`Desktop::nearest_valid`].

use ir_proto::ids::MonitorId;
use ir_proto::input::PointerPosition;
use ir_proto::screens::{Edge, MonitorInfo, ScreenLayout};

use crate::geom::{Point, Rect};

/// Um monitor validado.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Monitor {
    /// Identificador dentro deste arranjo.
    pub id: MonitorId,
    /// Onde ele fica, em pixels lógicos do desktop virtual.
    pub bounds: Rect,
    /// Escala em milésimos: `1000` é 100%.
    pub scale_permille: u16,
    /// Se é o monitor principal.
    pub primary: bool,
}

/// O conjunto de monitores de uma ponta.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Desktop {
    monitors: Vec<Monitor>,
    bounds: Rect,
    /// Cópia do monitor principal.
    ///
    /// Guardar o valor, e não um índice nem uma busca, torna o invariante "sempre existe um
    /// monitor principal" **estrutural**: não há caminho de código que precise entrar em
    /// pânico nem devolver `Option` para uma coisa que sempre existe.
    primary: Monitor,
}

impl Desktop {
    /// Constrói a partir de um arranjo anunciado.
    ///
    /// Devolve `None` para um arranjo vazio ou sem nenhum monitor de área positiva — o que
    /// é uma situação real (máquina sem tela, troca de monitor em andamento) e não um erro.
    /// Quem chama trata a ausência de desktop como "não há para onde mandar o ponteiro".
    #[must_use]
    pub fn from_layout(layout: &ScreenLayout) -> Option<Self> {
        let monitors: Vec<Monitor> = layout.monitors().iter().filter_map(Self::convert).collect();
        let bounds = monitors.iter().map(|m| m.bounds).reduce(Rect::union)?;
        let primary = *monitors
            .iter()
            .find(|m| m.primary)
            .or_else(|| monitors.first())?;
        Some(Self {
            monitors,
            bounds,
            primary,
        })
    }

    fn convert(info: &MonitorInfo) -> Option<Monitor> {
        Some(Monitor {
            id: info.id,
            bounds: Rect::new(info.x, info.y, info.width, info.height)?,
            scale_permille: if info.scale_permille == 0 {
                1000
            } else {
                info.scale_permille
            },
            primary: info.primary,
        })
    }

    /// O retângulo que envolve todos os monitores.
    ///
    /// Pode conter pontos que não pertencem a monitor nenhum, em arranjos não retangulares.
    #[must_use]
    pub const fn bounds(&self) -> Rect {
        self.bounds
    }

    /// Os monitores, na ordem em que foram anunciados.
    #[must_use]
    pub fn monitors(&self) -> &[Monitor] {
        &self.monitors
    }

    /// O monitor principal, ou o primeiro.
    ///
    /// Nunca devolve `None`: um `Desktop` só existe com pelo menos um monitor válido.
    #[must_use]
    pub const fn primary(&self) -> &Monitor {
        &self.primary
    }

    /// O monitor que contém o ponto, se algum contiver.
    #[must_use]
    pub fn monitor_at(&self, point: Point) -> Option<&Monitor> {
        self.monitors.iter().find(|m| m.bounds.contains(point))
    }

    /// O monitor com este identificador.
    #[must_use]
    pub fn monitor(&self, id: MonitorId) -> Option<&Monitor> {
        self.monitors.iter().find(|m| m.id == id)
    }

    /// O ponto válido mais próximo do dado.
    ///
    /// Se o ponto já está em algum monitor, devolve ele mesmo. Se não, devolve o ponto mais
    /// próximo dentro do monitor mais próximo — nunca uma coordenada que o injetor não possa
    /// usar. É o que mantém o produto utilizável quando um monitor é desconectado com o
    /// ponteiro em cima dele.
    #[must_use]
    pub fn nearest_valid(&self, point: Point) -> Point {
        if self.monitor_at(point).is_some() {
            return point;
        }
        self.monitors
            .iter()
            .map(|m| m.bounds.clamp(point))
            .min_by_key(|candidate| squared_distance(*candidate, point))
            .unwrap_or(point)
    }

    /// Converte um ponto do desktop na posição normalizada que viaja no protocolo.
    ///
    /// O ponto é primeiro trazido para dentro de um monitor, então esta função nunca falha.
    #[must_use]
    pub fn to_position(&self, point: Point) -> PointerPosition {
        let point = self.nearest_valid(point);
        let monitor = self.monitor_at(point).unwrap_or_else(|| self.primary());
        PointerPosition {
            monitor: monitor.id,
            x: monitor.bounds.fraction_x(point),
            y: monitor.bounds.fraction_y(point),
        }
    }

    /// Converte uma posição do protocolo num ponto deste desktop.
    ///
    /// Se o monitor anunciado não existe aqui — o par tem outro arranjo, ou o arranjo mudou
    /// entre o anúncio e a chegada da mensagem — a posição é interpretada contra o monitor
    /// principal. É melhor colocar o ponteiro na tela errada do que em nenhuma.
    #[must_use]
    pub fn from_position(&self, position: PointerPosition) -> Point {
        let monitor = self
            .monitor(position.monitor)
            .unwrap_or_else(|| self.primary());
        let bounds = monitor.bounds;
        Point {
            x: bounds.x_at(position.x),
            y: bounds.y_at(position.y),
        }
    }

    /// Se o ponto está na borda dada do desktop, pronto para atravessar.
    #[must_use]
    pub fn is_at_edge(&self, point: Point, edge: Edge) -> bool {
        match edge {
            Edge::Left => point.x <= self.bounds.left(),
            Edge::Right => point.x >= self.bounds.right(),
            Edge::Top => point.y <= self.bounds.top(),
            Edge::Bottom => point.y >= self.bounds.bottom(),
        }
    }
}

/// Distância ao quadrado, em `i64` para não estourar com coordenadas grandes.
fn squared_distance(a: Point, b: Point) -> i64 {
    let dx = i64::from(a.x) - i64::from(b.x);
    let dy = i64::from(a.y) - i64::from(b.y);
    dx * dx + dy * dy
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Monitor de teste. Por convenção, o de identificador 0 é o principal.
    fn info(id: u8, x: i32, y: i32, w: u32, h: u32) -> MonitorInfo {
        MonitorInfo {
            id: MonitorId(id),
            x,
            y,
            width: w,
            height: h,
            scale_permille: 1000,
            primary: id == 0,
        }
    }

    fn desktop(monitors: Vec<MonitorInfo>) -> Desktop {
        let layout = ScreenLayout::new(monitors).expect("arranjo válido");
        Desktop::from_layout(&layout).expect("desktop com monitor")
    }

    fn single() -> Desktop {
        desktop(vec![info(0, 0, 0, 1920, 1080)])
    }

    /// Dois monitores lado a lado, o segundo à esquerda em coordenada negativa.
    fn side_by_side() -> Desktop {
        desktop(vec![
            info(0, 0, 0, 1920, 1080),
            info(1, -1280, 0, 1280, 1024),
        ])
    }

    #[test]
    fn an_empty_layout_produces_no_desktop() {
        let layout = ScreenLayout::new(Vec::new()).expect("vazio é válido");
        assert!(
            Desktop::from_layout(&layout).is_none(),
            "sem tela não há desktop"
        );
    }

    #[test]
    fn bounds_cover_every_monitor() {
        let d = side_by_side();
        assert_eq!(d.bounds().left(), -1280);
        assert_eq!(d.bounds().right(), 1919);
        assert_eq!(d.bounds().height(), 1080, "a altura é a do maior");
        for monitor in d.monitors() {
            assert!(
                d.bounds()
                    .contains(Point::new(monitor.bounds.left(), monitor.bounds.top()))
            );
        }
    }

    #[test]
    fn primary_is_found_and_falls_back_to_the_first() {
        assert_eq!(single().primary().id, MonitorId(0));
        let no_primary = desktop(vec![info(3, 0, 0, 800, 600), info(4, 800, 0, 800, 600)]);
        assert_eq!(no_primary.primary().id, MonitorId(3));
    }

    #[test]
    fn monitor_at_finds_the_right_screen() {
        let d = side_by_side();
        assert_eq!(
            d.monitor_at(Point::new(10, 10)).map(|m| m.id),
            Some(MonitorId(0))
        );
        assert_eq!(
            d.monitor_at(Point::new(-10, 10)).map(|m| m.id),
            Some(MonitorId(1))
        );
        assert!(d.monitor_at(Point::new(-5000, 0)).is_none());
    }

    #[test]
    fn a_hole_in_an_l_shaped_arrangement_belongs_to_no_monitor() {
        // Um em cima à esquerda, outro embaixo à direita: o canto de cima à direita está
        // dentro de bounds e fora de qualquer tela.
        let d = desktop(vec![info(0, 0, 0, 800, 600), info(1, 800, 600, 800, 600)]);
        let hole = Point::new(1200, 100);
        assert!(
            d.bounds().contains(hole),
            "o buraco está dentro do retângulo envolvente"
        );
        assert!(
            d.monitor_at(hole).is_none(),
            "mas não pertence a monitor nenhum"
        );
    }

    #[test]
    fn nearest_valid_always_lands_on_a_real_monitor() {
        let d = desktop(vec![info(0, 0, 0, 800, 600), info(1, 800, 600, 800, 600)]);
        for point in [
            Point::new(1200, 100),
            Point::new(-9999, -9999),
            Point::new(9999, 9999),
            Point::new(0, 5000),
        ] {
            let fixed = d.nearest_valid(point);
            assert!(
                d.monitor_at(fixed).is_some(),
                "{point:?} virou {fixed:?}, ainda inválido"
            );
        }
    }

    #[test]
    fn nearest_valid_leaves_valid_points_alone() {
        let d = side_by_side();
        let inside = Point::new(100, 100);
        assert_eq!(d.nearest_valid(inside), inside);
    }

    #[test]
    fn nearest_valid_picks_the_closer_monitor() {
        let d = desktop(vec![info(0, 0, 0, 100, 100), info(1, 1000, 0, 100, 100)]);
        // Bem à esquerda: tem de ir para o monitor 0.
        assert_eq!(
            d.monitor_at(d.nearest_valid(Point::new(-500, 50)))
                .map(|m| m.id),
            Some(MonitorId(0))
        );
        // Bem à direita: tem de ir para o monitor 1.
        assert_eq!(
            d.monitor_at(d.nearest_valid(Point::new(5000, 50)))
                .map(|m| m.id),
            Some(MonitorId(1))
        );
    }

    #[test]
    fn position_round_trips_within_a_pixel() {
        let d = side_by_side();
        for point in [
            Point::new(0, 0),
            Point::new(1919, 1079),
            Point::new(960, 540),
            Point::new(-1280, 0),
            Point::new(-1, 500),
        ] {
            let position = d.to_position(point);
            let back = d.from_position(position);
            assert!(
                back.x.abs_diff(point.x) <= 1 && back.y.abs_diff(point.y) <= 1,
                "{point:?} voltou como {back:?}"
            );
        }
    }

    #[test]
    fn position_of_an_unknown_monitor_falls_back_to_primary() {
        let d = single();
        let alien = PointerPosition {
            monitor: MonitorId(200),
            x: 0,
            y: 0,
        };
        let point = d.from_position(alien);
        assert!(
            d.monitor_at(point).is_some(),
            "tem de cair em alguma tela real"
        );
        assert_eq!(d.monitor_at(point).map(|m| m.id), Some(d.primary().id));
    }

    #[test]
    fn to_position_never_produces_an_unusable_coordinate() {
        let d = desktop(vec![info(0, 0, 0, 800, 600), info(1, 800, 600, 800, 600)]);
        for point in [
            Point::new(1200, 100),
            Point::new(-9999, 0),
            Point::new(i32::MAX, i32::MIN),
        ] {
            let position = d.to_position(point);
            assert!(
                d.monitor(position.monitor).is_some(),
                "monitor inexistente para {point:?}"
            );
            let back = d.from_position(position);
            assert!(
                d.monitor_at(back).is_some(),
                "{point:?} virou coordenada inválida"
            );
        }
    }

    #[test]
    fn edge_detection_uses_the_whole_desktop_not_one_monitor() {
        let d = side_by_side();
        assert!(d.is_at_edge(Point::new(1919, 500), Edge::Right));
        assert!(
            !d.is_at_edge(Point::new(0, 500), Edge::Right),
            "borda do monitor não é do desktop"
        );
        assert!(d.is_at_edge(Point::new(-1280, 500), Edge::Left));
        assert!(d.is_at_edge(Point::new(500, 0), Edge::Top));
        assert!(d.is_at_edge(Point::new(500, 1079), Edge::Bottom));
    }

    #[test]
    fn edge_detection_treats_beyond_the_edge_as_at_the_edge() {
        // O gancho pode entregar um delta que passa da borda de uma vez.
        let d = single();
        assert!(d.is_at_edge(Point::new(5000, 500), Edge::Right));
        assert!(d.is_at_edge(Point::new(-5000, 500), Edge::Left));
    }

    #[test]
    fn a_zero_area_monitor_is_dropped_instead_of_breaking_the_desktop() {
        // ScreenLayout já recusa isto, mas o desktop não pode depender disso: a defesa em
        // profundidade é o que impede divisão por zero se a validação mudar.
        let broken = MonitorInfo {
            id: MonitorId(9),
            x: 0,
            y: 0,
            width: 0,
            height: 0,
            scale_permille: 0,
            primary: false,
        };
        assert!(Desktop::convert(&broken).is_none());
    }

    #[test]
    fn a_zero_scale_is_normalised_to_one_hundred_percent() {
        let odd = MonitorInfo {
            id: MonitorId(1),
            x: 0,
            y: 0,
            width: 100,
            height: 100,
            scale_permille: 0,
            primary: true,
        };
        assert_eq!(Desktop::convert(&odd).map(|m| m.scale_permille), Some(1000));
    }
}
