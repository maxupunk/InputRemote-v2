//! A borda de travessia: dos dois lados, a escolha mais recente vale (ADR-0014).
//!
//! No teste físico (log 24), cada máquina tinha a própria escolha de borda. O usuário trocou dos
//! dois lados, as duas ficaram com `left`, e a volta passou a sair pelo lado errado. Depois a borda
//! passou a ser só do servidor; sem papel fixo, qualquer um dos dois escolhe, e o outro passa a
//! usar a oposta. Trocar a borda ajusta a sessão em uso, sem derrubá-la.
//!
//! Na bancada, `server` é o computador A (identificador 1) e `client` o B (identificador 2): nomes
//! de posição, não de papel.
#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

mod common;

use common::{Pair, Side, is};
use ir_proto::carrier::Carrier;
use ir_proto::channel::ChannelId;
use ir_proto::frame::Frame;
use ir_proto::input::{HidUsage, PointerDelta};
use ir_proto::message::{Control, Message};
use ir_proto::screens::Edge;
use ir_session::event::Notice;
use ir_session::{Command, Input, Phase};

fn disconnections(pair: &Pair, side: Side) -> usize {
    pair.notices(side)
        .iter()
        .filter(|notice| matches!(notice, Notice::Disconnected { .. }))
        .count()
}

/// As bordas que um lado passou a usar porque o par escolheu — o que o serviço grava e conta.
fn edges_adopted(pair: &Pair, side: Side) -> Vec<Edge> {
    pair.notices(side)
        .iter()
        .filter_map(|notice| match notice {
            Notice::EdgeAdopted { edge, .. } => Some(*edge),
            _ => None,
        })
        .collect()
}

/// As bordas escolhidas na própria tela.
fn edges_chosen(pair: &Pair, side: Side) -> Vec<Edge> {
    pair.notices(side)
        .iter()
        .filter_map(|notice| match notice {
            Notice::EdgeChanged { edge, .. } => Some(*edge),
            _ => None,
        })
        .collect()
}

fn set_edge(pair: &mut Pair, side: Side, edge: Edge, chosen_at: u64) {
    pair.feed(side, Input::SetPeerEdge { edge, chosen_at });
}

fn connected() -> Pair {
    let mut pair = Pair::matched();
    pair.connect(Carrier::Udp);
    assert_eq!(pair.client.phase(), Phase::Ready);
    pair.clear_log();
    pair
}

#[test]
fn two_edges_never_chosen_agree_by_the_smaller_identifier() {
    // A bancada do log 24: as duas máquinas tinham gravado `left`, nenhuma escolhida pela tela.
    let mut pair = Pair::with_edges(Edge::Left, Edge::Left);
    pair.connect(Carrier::Udp);

    assert_eq!(
        pair.server.peer_edge(),
        Edge::Left,
        "o menor identificador fica"
    );
    assert_eq!(
        pair.client.peer_edge(),
        Edge::Right,
        "B fica à esquerda de A, então A fica à direita de B"
    );
    assert_eq!(edges_adopted(&pair, Side::Client), vec![Edge::Right]);
    assert!(edges_adopted(&pair, Side::Server).is_empty());
}

#[test]
fn edges_that_already_agree_have_nothing_to_save() {
    let mut pair = Pair::matched();
    pair.connect(Carrier::Udp);

    assert_eq!(pair.client.peer_edge(), Edge::Left);
    assert!(edges_adopted(&pair, Side::Client).is_empty());
    assert!(edges_adopted(&pair, Side::Server).is_empty());
}

#[test]
fn changing_the_edge_on_one_side_keeps_the_session_up_and_the_other_follows() {
    let mut pair = connected();

    set_edge(&mut pair, Side::Server, Edge::Left, 100);
    for _ in 0..20 {
        pair.advance(100);
    }

    assert_eq!(pair.server.peer_edge(), Edge::Left);
    assert_eq!(
        pair.client.peer_edge(),
        Edge::Right,
        "o outro acompanha sem ninguém mexer nele"
    );
    assert_eq!(pair.server.phase(), Phase::Ready);
    assert_eq!(pair.client.phase(), Phase::Ready);
    assert_eq!(disconnections(&pair, Side::Server), 0);
    assert_eq!(disconnections(&pair, Side::Client), 0);
    assert_eq!(edges_chosen(&pair, Side::Server), vec![Edge::Left]);
    assert_eq!(edges_adopted(&pair, Side::Client), vec![Edge::Right]);
}

#[test]
fn either_side_may_choose_and_the_other_follows() {
    // O que antes era proibido: o computador que "era controlado" também escolhe.
    let mut pair = connected();

    set_edge(&mut pair, Side::Client, Edge::Top, 100);

    assert_eq!(pair.client.peer_edge(), Edge::Top);
    assert_eq!(
        pair.server.peer_edge(),
        Edge::Bottom,
        "A fica abaixo de B, então B fica abaixo de A"
    );
    assert_eq!(edges_adopted(&pair, Side::Server), vec![Edge::Bottom]);
}

#[test]
fn when_both_change_the_latest_choice_wins() {
    let mut pair = connected();
    set_edge(&mut pair, Side::Server, Edge::Top, 200);
    set_edge(&mut pair, Side::Client, Edge::Right, 300);

    assert_eq!(
        pair.client.peer_edge(),
        Edge::Right,
        "a escolha mais recente fica"
    );
    assert_eq!(pair.server.peer_edge(), Edge::Left);
}

#[test]
fn after_the_change_the_new_edge_is_the_one_that_crosses() {
    let mut pair = connected();
    set_edge(&mut pair, Side::Server, Edge::Left, 100);

    // A direita, que era a borda antiga, agora prende o ponteiro.
    pair.feed(
        Side::Server,
        Input::LocalPointer(PointerDelta { dx: 5000, dy: 0 }),
    );
    assert_eq!(
        pair.server.phase(),
        Phase::Ready,
        "a borda antiga não atravessa mais"
    );

    pair.feed(
        Side::Server,
        Input::LocalPointer(PointerDelta { dx: -5000, dy: 0 }),
    );
    assert_eq!(
        pair.server.phase(),
        Phase::Sending,
        "a borda nova atravessa"
    );
    assert_eq!(pair.client.phase(), Phase::Receiving);

    // A volta sai pela borda oposta do outro lado: a direita dele.
    pair.feed(
        Side::Server,
        Input::LocalPointer(PointerDelta { dx: 5000, dy: 0 }),
    );
    pair.advance(20);
    assert_eq!(
        pair.server.phase(),
        Phase::Ready,
        "o controle volta pela direita do outro"
    );
}

#[test]
fn changing_the_edge_while_controlling_hands_control_back_first() {
    let mut pair = connected();
    pair.feed(
        Side::Server,
        Input::LocalPointer(PointerDelta { dx: 5000, dy: 0 }),
    );
    pair.feed(
        Side::Server,
        Input::LocalKey {
            usage: HidUsage(0x04),
            pressed: true,
        },
    );
    assert_eq!(pair.client.phase(), Phase::Receiving);
    pair.clear_log();

    set_edge(&mut pair, Side::Server, Edge::Left, 100);

    assert_eq!(
        pair.server.phase(),
        Phase::Ready,
        "o controle volta antes de a borda mudar"
    );
    assert_eq!(pair.client.phase(), Phase::Ready);
    assert!(
        pair.any(Side::Client, is::release_all),
        "a tecla pressionada do outro lado precisa ser solta"
    );
    assert!(pair.client.input_state().is_released());
    assert!(
        pair.any(Side::Server, is::unsuppress),
        "a entrada local precisa voltar"
    );
    let released = pair.index_of_release(Side::Client).expect("soltou");
    let adopted = pair
        .index_of(Side::Client, |command| {
            matches!(command, Command::Notify(Notice::EdgeAdopted { .. }))
        })
        .expect("acompanhou a borda nova");
    assert!(
        released < adopted,
        "primeiro soltar, depois qualquer outra coisa"
    );
    assert_eq!(disconnections(&pair, Side::Server), 0);
    assert_eq!(disconnections(&pair, Side::Client), 0);
}

#[test]
fn an_older_announcement_does_not_undo_a_newer_choice() {
    // Um anúncio atrasado, ou de alguém que escolheu antes, não desfaz o que se escolheu aqui.
    let mut pair = connected();
    set_edge(&mut pair, Side::Server, Edge::Left, 500);
    pair.advance(210);
    let epoch = pair
        .commands(Side::Client)
        .into_iter()
        .find_map(|command| match command {
            Command::Send { frame, .. } => Some(frame.epoch),
            _ => None,
        })
        .expect("o heartbeat do outro lado mostra a época da sessão");
    let seq = pair.client.next_sequence(ChannelId::Control);
    let stale = Frame::new(
        Message::Control(Control::EdgeConfig {
            peer_edge: Edge::Top,
            chosen_at: 100,
        }),
        seq,
    )
    .in_epoch(epoch);
    pair.clear_log();

    pair.feed(
        Side::Server,
        Input::Received {
            carrier: Carrier::Udp,
            frame: stale,
        },
    );

    assert_eq!(pair.server.peer_edge(), Edge::Left);
    assert!(edges_adopted(&pair, Side::Server).is_empty());
}

#[test]
fn sem_posicao_real_o_ponteiro_comeca_do_lado_oposto_a_borda() {
    // O Linux não sabe onde o cursor está. Semeado no meio, o modelo atravessava com o cursor real
    // longe da borda (log 49); do lado oposto, a travessia só vem depois dele.
    let mut pair = Pair::matched();
    pair.server.seed_pointer_away_from_edge();
    let (x, y) = pair.server.pointer_xy();
    assert!(
        x <= 1,
        "a borda do par é a direita: começa encostado na esquerda ({x})"
    );
    assert!((500..580).contains(&y), "no meio da altura: {y}");
}
