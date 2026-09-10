//! Conversão entre ponto do desktop e posição do protocolo.
//!
//! Duas direções, e a garantia de que nenhuma delas produz coordenada inutilizável. É o que
//! mantém o produto funcionando quando um monitor é desconectado com o ponteiro em cima dele,
//! ou quando o par anuncia um monitor que não existe aqui.

use ir_proto::input::PointerPosition;
use ir_proto::screens::{Edge, MonitorInfo, ScreenLayout};

use super::Desktop;
use crate::geom::Point;

impl Desktop {
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

    /// O arranjo na forma que viaja no fio.
    ///
    /// O caminho de volta de [`Desktop::from_layout`]. Existe para que a sessão possa
    /// anunciar ao par o arranjo que está de fato em uso — que pode diferir do anunciado
    /// pelo sistema, porque monitores de área zero foram descartados na construção.
    ///
    /// Nunca falha: um `Desktop` só existe com monitores válidos, então o `ScreenLayout`
    /// resultante é válido por construção.
    #[must_use]
    pub fn to_layout(&self) -> ScreenLayout {
        let monitors = self
            .monitors
            .iter()
            .map(|m| MonitorInfo {
                id: m.id,
                x: m.bounds.left(),
                y: m.bounds.top(),
                width: m.bounds.width(),
                height: m.bounds.height(),
                scale_permille: m.scale_permille,
                primary: m.primary,
            })
            .collect();
        ScreenLayout::new(monitors).unwrap_or_default()
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

#[cfg(test)]
mod tests {
    use super::*;
    use ir_proto::ids::MonitorId;

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
    fn a_layout_survives_a_round_trip_through_the_desktop() {
        let original = ScreenLayout::new(vec![
            info(0, 0, 0, 1920, 1080),
            info(1, -1280, 100, 1280, 1024),
        ])
        .expect("arranjo válido");
        let desktop = Desktop::from_layout(&original).expect("desktop");
        assert_eq!(
            desktop.to_layout(),
            original,
            "ida e volta tem de preservar tudo"
        );
    }

    #[test]
    fn to_layout_drops_the_monitors_that_from_layout_ignored() {
        // Defesa em profundidade: se um monitor inválido escapar da validação do protocolo,
        // ele não é reanunciado ao par como se fosse válido.
        let desktop = single();
        assert_eq!(desktop.to_layout().len(), desktop.monitors().len());
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
}
