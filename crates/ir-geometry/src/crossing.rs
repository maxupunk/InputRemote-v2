//! Travessia de borda: quando o controle passa, e onde o ponteiro aparece do outro lado.
//!
//! O produto tem **uma** borda ativa por sessão, a que dá para o computador par. As outras
//! três seguram o ponteiro. Essa assimetria é deliberada: um KVM em que qualquer borda
//! atravessa é um KVM que rouba o controle quando o usuário mira num botão de canto.

use ir_proto::input::{PointerDelta, PointerPosition};
use ir_proto::screens::Edge;

use crate::desktop::Desktop;
use crate::geom::Point;

/// Uma travessia detectada.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Crossing {
    /// Por qual borda o ponteiro saiu.
    pub exit_edge: Edge,
    /// Onde ao longo dessa borda, normalizado em `0..=u16::MAX`.
    ///
    /// Fração e não pixel: é o que faz "no meio da borda" continuar sendo no meio do outro
    /// lado, mesmo com monitores de resoluções diferentes.
    pub fraction: u16,
}

/// O resultado de mover o ponteiro dentro de um desktop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Movement {
    /// O ponteiro continua neste desktop, nesta posição.
    Stayed(Point),
    /// O ponteiro atravessou para o par.
    Crossed(Crossing),
}

/// Aplica um deslocamento e diz se houve travessia.
///
/// Só a borda `peer_edge` atravessa; nas outras três o ponteiro é preso, como um monitor
/// isolado faria. O ponto de partida é trazido para dentro do desktop antes de tudo, para
/// que um estado herdado de um arranjo antigo não produza travessia falsa.
#[must_use]
pub fn advance(desktop: &Desktop, from: Point, delta: PointerDelta, peer_edge: Edge) -> Movement {
    let start = desktop.nearest_valid(from);
    let target = start.offset(delta.dx, delta.dy);

    if crosses(desktop, start, target, peer_edge) {
        let bounds = desktop.bounds();
        // A fração é medida no ponto de saída projetado sobre a borda: usar `target` e não
        // `start` faz um movimento diagonal rápido entrar na altura certa do outro lado.
        return Movement::Crossed(Crossing {
            exit_edge: peer_edge,
            fraction: bounds.fraction_along(peer_edge, target),
        });
    }

    Movement::Stayed(desktop.nearest_valid(target))
}

/// Se o movimento de `start` para `target` sai pela borda dada.
///
/// Exige que o ponto de partida **não** estivesse já fora: assim uma sequência de deltas na
/// mesma direção não dispara travessia repetida, e o ponteiro fica preso na borda até que o
/// usuário empurre de novo a partir de dentro.
fn crosses(desktop: &Desktop, start: Point, target: Point, edge: Edge) -> bool {
    if desktop.is_at_edge(start, edge) {
        // Já estava na borda: só atravessa se o movimento insistir para fora.
        return pushes_outward(start, target, edge);
    }
    desktop.is_at_edge(target, edge) && pushes_outward(start, target, edge)
}

const fn pushes_outward(start: Point, target: Point, edge: Edge) -> bool {
    match edge {
        Edge::Left => target.x < start.x,
        Edge::Right => target.x > start.x,
        Edge::Top => target.y < start.y,
        Edge::Bottom => target.y > start.y,
    }
}

/// Onde o ponteiro aparece no desktop de destino, dada uma travessia.
///
/// Entra pela borda **oposta** à de saída: sair pela direita de uma tela é entrar pela
/// esquerda da outra. O ponto vem recuado um pixel para dentro, o que impede o ping-pong de
/// entrar e sair na mesma amostra.
#[must_use]
pub fn entry_position(destination: &Desktop, crossing: Crossing) -> PointerPosition {
    let entering = crossing.exit_edge.opposite();
    let point = destination
        .bounds()
        .point_along(entering, crossing.fraction);
    destination.to_position(destination.nearest_valid(point))
}

/// A borda por onde o ponteiro entra no destino.
#[must_use]
pub const fn entering_edge(crossing: Crossing) -> Edge {
    crossing.exit_edge.opposite()
}

#[cfg(test)]
mod tests {
    use super::*;
    use ir_proto::ids::MonitorId;
    use ir_proto::screens::{MonitorInfo, ScreenLayout};

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
        Desktop::from_layout(&ScreenLayout::new(monitors).expect("arranjo")).expect("desktop")
    }

    fn hd() -> Desktop {
        desktop(vec![info(0, 0, 0, 1920, 1080)])
    }

    fn small() -> Desktop {
        desktop(vec![info(0, 0, 0, 1280, 720)])
    }

    fn delta(dx: i32, dy: i32) -> PointerDelta {
        PointerDelta { dx, dy }
    }

    #[test]
    fn moving_inside_never_crosses() {
        let d = hd();
        let moved = advance(&d, Point::new(500, 500), delta(10, 10), Edge::Right);
        assert_eq!(moved, Movement::Stayed(Point::new(510, 510)));
    }

    #[test]
    fn reaching_the_peer_edge_crosses() {
        let d = hd();
        let moved = advance(&d, Point::new(1900, 540), delta(100, 0), Edge::Right);
        match moved {
            Movement::Crossed(crossing) => assert_eq!(crossing.exit_edge, Edge::Right),
            Movement::Stayed(point) => panic!("deveria ter atravessado, ficou em {point:?}"),
        }
    }

    #[test]
    fn the_other_three_edges_hold_the_pointer() {
        let d = hd();
        for edge in [Edge::Left, Edge::Top, Edge::Bottom] {
            let (from, push) = match edge {
                Edge::Left => (Point::new(5, 540), delta(-100, 0)),
                Edge::Top => (Point::new(960, 5), delta(0, -100)),
                _ => (Point::new(960, 1075), delta(0, 100)),
            };
            // A borda do par é a direita; as outras não atravessam.
            let moved = advance(&d, from, push, Edge::Right);
            match moved {
                Movement::Stayed(point) => {
                    assert!(
                        d.monitor_at(point).is_some(),
                        "{edge} deixou coordenada inválida"
                    );
                }
                Movement::Crossed(_) => panic!("{edge} não deveria atravessar"),
            }
        }
    }

    #[test]
    fn sitting_at_the_edge_without_pushing_does_not_cross() {
        let d = hd();
        // Já na borda, movendo só na vertical: não insiste para fora.
        let moved = advance(&d, Point::new(1919, 100), delta(0, 50), Edge::Right);
        assert!(
            matches!(moved, Movement::Stayed(_)),
            "movimento paralelo não atravessa"
        );
    }

    #[test]
    fn sitting_at_the_edge_and_pushing_outward_crosses() {
        let d = hd();
        let moved = advance(&d, Point::new(1919, 100), delta(1, 0), Edge::Right);
        assert!(matches!(moved, Movement::Crossed(_)));
    }

    #[test]
    fn moving_back_from_the_edge_does_not_cross() {
        let d = hd();
        let moved = advance(&d, Point::new(1919, 100), delta(-50, 0), Edge::Right);
        assert_eq!(moved, Movement::Stayed(Point::new(1869, 100)));
    }

    #[test]
    fn a_huge_delta_crosses_instead_of_overflowing() {
        let d = hd();
        let moved = advance(&d, Point::new(960, 540), delta(i32::MAX, 0), Edge::Right);
        assert!(matches!(moved, Movement::Crossed(_)));
    }

    #[test]
    fn a_huge_inward_delta_clamps_instead_of_overflowing() {
        let d = hd();
        let moved = advance(
            &d,
            Point::new(960, 540),
            delta(i32::MIN, i32::MIN),
            Edge::Right,
        );
        match moved {
            Movement::Stayed(point) => assert!(d.monitor_at(point).is_some()),
            Movement::Crossed(_) => panic!("a borda esquerda não é a do par"),
        }
    }

    #[test]
    fn entry_comes_in_through_the_opposite_edge() {
        for exit in Edge::ALL {
            let crossing = Crossing {
                exit_edge: exit,
                fraction: 0,
            };
            assert_eq!(entering_edge(crossing), exit.opposite());
        }
    }

    #[test]
    fn entry_lands_on_a_real_monitor_for_every_edge_and_fraction() {
        let d = desktop(vec![info(0, 0, 0, 800, 600), info(1, 800, 600, 800, 600)]);
        for exit in Edge::ALL {
            for fraction in [0u16, 1, 12_345, u16::MAX / 2, u16::MAX] {
                let position = entry_position(
                    &d,
                    Crossing {
                        exit_edge: exit,
                        fraction,
                    },
                );
                assert!(
                    d.monitor(position.monitor).is_some(),
                    "{exit}/{fraction} apontou para monitor inexistente"
                );
                let point = d.from_position(position);
                assert!(
                    d.monitor_at(point).is_some(),
                    "{exit}/{fraction} caiu fora de toda tela: {point:?}"
                );
            }
        }
    }

    #[test]
    fn the_middle_of_one_edge_is_the_middle_of_the_other() {
        // Resoluções diferentes nos dois lados: a proporção tem de ser preservada, que é a
        // razão de a travessia usar fração e não pixel.
        let source = hd();
        let destination = small();
        let moved = advance(&source, Point::new(1900, 540), delta(100, 0), Edge::Right);
        let Movement::Crossed(crossing) = moved else {
            panic!("deveria atravessar")
        };

        let position = entry_position(&destination, crossing);
        let point = destination.from_position(position);
        let middle = i32::try_from(destination.bounds().height() / 2).expect("altura cabe");
        assert!(
            point.y.abs_diff(middle) < 20,
            "entrou em y={} quando o meio é {middle}",
            point.y
        );
    }

    #[test]
    fn entry_is_never_on_the_far_edge_so_it_cannot_bounce_back() {
        // Se o ponto de entrada caísse exatamente na borda de saída do destino, a próxima
        // amostra atravessaria de volta — o ping-pong.
        let d = hd();
        for exit in Edge::ALL {
            let entering = exit.opposite();
            let position = entry_position(
                &d,
                Crossing {
                    exit_edge: exit,
                    fraction: 30_000,
                },
            );
            let point = d.from_position(position);
            assert!(
                !d.is_at_edge(point, entering),
                "{exit}: entrou exatamente na borda {entering}, vai quicar"
            );
        }
    }

    #[test]
    fn a_round_trip_across_the_border_returns_near_the_departure_height() {
        let a = hd();
        let b = small();

        let out = advance(&a, Point::new(1900, 800), delta(100, 0), Edge::Right);
        let Movement::Crossed(going) = out else {
            panic!("ida deveria atravessar")
        };
        let landed = b.from_position(entry_position(&b, going));

        // No cliente, o par fica à esquerda: empurrar para a esquerda devolve o controle.
        let back = advance(
            &b,
            landed,
            delta(-i32::try_from(b.bounds().width()).unwrap_or(i32::MAX), 0),
            Edge::Left,
        );
        let Movement::Crossed(returning) = back else {
            panic!("volta deveria atravessar")
        };
        let home = a.from_position(entry_position(&a, returning));

        let departure_fraction = a
            .bounds()
            .fraction_along(Edge::Right, Point::new(1919, 800));
        let arrival_fraction = a.bounds().fraction_along(Edge::Right, home);
        let drift = u32::from(departure_fraction).abs_diff(u32::from(arrival_fraction));
        assert!(
            drift < u32::from(u16::MAX) / 20,
            "a ida e volta deslocou {drift} de {} — mais de 5%",
            u16::MAX
        );
    }

    #[test]
    fn a_stale_starting_point_does_not_produce_a_false_crossing() {
        // Cenário real: o monitor foi desconectado e a posição guardada ficou fora de tudo.
        let d = hd();
        let moved = advance(&d, Point::new(9999, 9999), delta(0, 0), Edge::Right);
        match moved {
            Movement::Stayed(point) => assert!(d.monitor_at(point).is_some()),
            Movement::Crossed(_) => panic!("posição obsoleta não deve atravessar sozinha"),
        }
    }
}
