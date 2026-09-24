//! Cenários de travessia de borda: ida, volta, geometria e coalescência.
//!
//! Linhas correspondentes de `docs/10-testes-e-validacao.md` §2: travessia ida e volta,
//! geometrias diferentes nos dois lados, e monitor removido durante a sessão.
#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

mod common;

use common::{Pair, Side, is, layout};
use ir_proto::carrier::Carrier;
use ir_proto::input::{HidUsage, PointerDelta};
use ir_session::{Input, Phase};

/// Leva o ponteiro do servidor até a borda direita e atravessa.
fn cross_to_client(pair: &mut Pair) {
    pair.feed(
        Side::Server,
        Input::LocalPointer(PointerDelta { dx: 5000, dy: 0 }),
    );
}

fn connected() -> Pair {
    let mut pair = Pair::matched();
    pair.connect(Carrier::Udp);
    pair.clear_log();
    pair
}

#[test]
fn crossing_the_edge_hands_control_over_and_suppresses_local_input() {
    let mut pair = connected();
    cross_to_client(&mut pair);

    assert_eq!(
        pair.server.phase(),
        Phase::Sending,
        "o servidor entregou o controle"
    );
    assert_eq!(pair.client.phase(), Phase::Receiving, "o cliente assumiu");
    assert!(pair.any(Side::Server, is::enter_screen));
    assert!(
        pair.any(Side::Server, is::suppress),
        "a entrada local precisa parar"
    );
    assert!(
        pair.any(Side::Client, is::injection),
        "o ponteiro precisa aparecer no cliente"
    );
}

#[test]
fn only_the_peer_edge_crosses() {
    let mut pair = connected();
    // A borda do par é a direita; empurrar para a esquerda não pode atravessar.
    pair.feed(
        Side::Server,
        Input::LocalPointer(PointerDelta { dx: -5000, dy: 0 }),
    );
    assert_eq!(
        pair.server.phase(),
        Phase::Ready,
        "a borda esquerda prende o ponteiro"
    );
    assert!(!pair.any(Side::Server, is::enter_screen));
}

#[test]
fn a_round_trip_across_the_border_returns_control_and_releases_everything() {
    let mut pair = connected();
    cross_to_client(&mut pair);
    pair.feed(
        Side::Server,
        Input::LocalKey {
            usage: HidUsage(0x04),
            pressed: true,
        },
    );
    pair.clear_log();

    // No cliente, o servidor está à esquerda: empurrar para lá devolve o controle.
    // O movimento é coalescido e despachado no intervalo, então o relógio precisa andar —
    // é o mesmo caminho que a periferia percorre ao honrar o temporizador de despacho.
    pair.feed(
        Side::Server,
        Input::LocalPointer(PointerDelta { dx: -5000, dy: 0 }),
    );
    pair.advance(20);

    assert_eq!(pair.server.phase(), Phase::Ready, "o controle voltou");
    assert_eq!(pair.client.phase(), Phase::Ready);
    assert!(
        pair.any(Side::Client, is::release_all),
        "nada pode ficar pressionado"
    );
    assert!(pair.client.input_state().is_released());
    assert!(
        pair.any(Side::Server, is::unsuppress),
        "a entrada local precisa voltar"
    );
    assert!(
        pair.any(Side::Server, is::warp),
        "o cursor precisa reaparecer na borda"
    );
}

#[test]
fn crossing_between_different_resolutions_lands_proportionally() {
    // Servidor 1920×1080, cliente 1280×720: sair na metade da altura tem de entrar na metade.
    let mut pair = Pair::new(layout(1920, 1080), layout(1280, 720));
    pair.connect(Carrier::Udp);
    // Põe o ponteiro na metade vertical antes de atravessar.
    pair.feed(
        Side::Server,
        Input::LocalPointer(PointerDelta { dx: 100, dy: 540 }),
    );
    pair.clear_log();
    cross_to_client(&mut pair);

    assert_eq!(pair.client.phase(), Phase::Receiving);
    let landed = pair.client.pointer();
    let middle = 720 / 2;
    assert!(
        (landed.y - middle).abs() < 40,
        "entrou em y={} quando o meio é {middle}",
        landed.y
    );
}

#[test]
fn a_monitor_removed_mid_session_never_produces_an_invalid_coordinate() {
    let mut pair = Pair::new(common::dual_layout(), layout(1920, 1080));
    pair.connect(Carrier::Udp);
    // Leva o ponteiro para o segundo monitor.
    pair.feed(
        Side::Server,
        Input::LocalPointer(PointerDelta { dx: 2500, dy: 500 }),
    );
    pair.clear_log();

    // O segundo monitor é desconectado com o ponteiro em cima dele.
    pair.feed(Side::Server, Input::LocalScreens(layout(1920, 1080)));

    let pointer = pair.server.pointer();
    assert!(
        (0..1920).contains(&pointer.x) && (0..1080).contains(&pointer.y),
        "o ponteiro ficou em {pointer:?}, fora de toda tela"
    );
}

#[test]
fn a_pointer_move_while_remote_is_coalesced_and_dispatched_on_the_interval() {
    let mut pair = connected();
    cross_to_client(&mut pair);
    pair.clear_log();

    // Três amostras seguidas, sem o relógio andar: nada sai ainda.
    for _ in 0..3 {
        pair.feed(
            Side::Server,
            Input::LocalPointer(PointerDelta { dx: 3, dy: 0 }),
        );
    }
    assert_eq!(
        pair.count(Side::Client, is::injection),
        0,
        "sem o intervalo vencer, o movimento fica acumulado"
    );

    pair.advance(20);
    let injections = pair.count(Side::Client, is::injection);
    assert_eq!(injections, 1, "as três amostras viram uma só, somada");
}

#[test]
fn seeding_the_pointer_positions_without_crossing_then_a_delta_crosses() {
    // Reproduz o cenário da máquina real: o cursor está perto da borda direita, e o servidor
    // precisa saber a posição **absoluta** para atravessar no ponto certo. Semear não atravessa;
    // um movimento pequeno a partir dali, sim. Sem a semeadura, o servidor acumularia deltas de
    // (0,0) e o cursor real chegaria à borda física da tela antes de o modelo chegar à sua — e a
    // travessia nunca aconteceria.
    let mut pair = connected();

    pair.server.sync_pointer(1900, 500);
    assert_eq!(
        pair.server.phase(),
        Phase::Ready,
        "semear a posição não pode atravessar"
    );
    assert_eq!(pair.server.pointer_xy(), (1900, 500));

    pair.feed(
        Side::Server,
        Input::LocalPointer(PointerDelta { dx: 100, dy: 0 }),
    );
    assert_eq!(
        pair.server.phase(),
        Phase::Sending,
        "chegando na borda a partir da posição real, atravessa"
    );
}

#[test]
fn syncing_clamps_a_position_outside_every_screen() {
    let mut pair = connected();
    pair.server.sync_pointer(-500, -500);
    let (x, y) = pair.server.pointer_xy();
    assert!(
        x >= 0 && y >= 0,
        "posição fora de toda tela é trazida para dentro"
    );
}
