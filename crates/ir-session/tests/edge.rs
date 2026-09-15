//! A borda de travessia: o servidor decide, o cliente acompanha.
//!
//! No teste físico (log 24), cada máquina tinha a própria escolha de borda. O usuário trocou dos
//! dois lados, as duas ficaram com `left`, e a volta passou a sair pelo lado errado do cliente. E
//! cada troca derrubava a sessão, com um adeus que ainda mandava o par não reconectar.
//!
//! A regra agora: **a fonte de verdade é o servidor**, que tem o teclado e o mouse. O cliente usa
//! sempre a borda oposta à dele — se o cliente fica à esquerda do servidor, o servidor fica à
//! direita do cliente —, e trocar a borda ajusta a sessão em uso, sem derrubá-la.
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

/// As bordas que um lado anunciou ter passado a usar — o que o serviço grava.
fn edges_adopted(pair: &Pair, side: Side) -> Vec<Edge> {
    pair.notices(side)
        .iter()
        .filter_map(|notice| match notice {
            Notice::EdgeChanged { edge } => Some(*edge),
            _ => None,
        })
        .collect()
}

fn connected() -> Pair {
    let mut pair = Pair::matched();
    pair.connect(Carrier::Udp);
    assert_eq!(pair.client.phase(), Phase::Ready);
    pair.clear_log();
    pair
}

#[test]
fn the_client_takes_the_opposite_of_the_servers_edge_when_the_session_starts() {
    // A bancada do log 24: as duas máquinas tinham gravado `left`.
    let mut pair = Pair::with_edges(Edge::Left, Edge::Left);
    pair.connect(Carrier::Udp);

    assert_eq!(pair.client.phase(), Phase::Ready);
    assert_eq!(
        pair.client.peer_edge(),
        Edge::Right,
        "o cliente fica à esquerda do servidor, então o servidor fica à direita do cliente"
    );
    assert_eq!(
        edges_adopted(&pair, Side::Client),
        vec![Edge::Right],
        "o serviço do cliente precisa saber que borda gravar"
    );
}

#[test]
fn a_client_that_already_agrees_has_nothing_to_save() {
    let mut pair = Pair::matched();
    pair.connect(Carrier::Udp);

    assert_eq!(pair.client.peer_edge(), Edge::Left);
    assert!(
        edges_adopted(&pair, Side::Client).is_empty(),
        "sem mudança, nada a gravar a cada conexão"
    );
}

#[test]
fn changing_the_edge_on_the_server_keeps_the_session_up() {
    let mut pair = connected();

    pair.feed(Side::Server, Input::SetPeerEdge(Edge::Left));
    for _ in 0..20 {
        pair.advance(100);
    }

    assert_eq!(pair.server.peer_edge(), Edge::Left);
    assert_eq!(
        pair.client.peer_edge(),
        Edge::Right,
        "o cliente acompanha sem ninguém mexer nele"
    );
    assert_eq!(pair.server.phase(), Phase::Ready);
    assert_eq!(pair.client.phase(), Phase::Ready);
    assert_eq!(
        disconnections(&pair, Side::Server),
        0,
        "trocar a borda não é motivo para derrubar a sessão"
    );
    assert_eq!(disconnections(&pair, Side::Client), 0);
    assert_eq!(edges_adopted(&pair, Side::Server), vec![Edge::Left]);
    assert_eq!(edges_adopted(&pair, Side::Client), vec![Edge::Right]);
}

#[test]
fn after_the_change_the_new_edge_is_the_one_that_crosses() {
    let mut pair = connected();
    pair.feed(Side::Server, Input::SetPeerEdge(Edge::Left));

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
        Phase::Engaged,
        "a borda nova atravessa"
    );
    assert_eq!(pair.client.phase(), Phase::Engaged);

    // A volta sai pela borda oposta do cliente: a direita dele.
    pair.feed(
        Side::Server,
        Input::LocalPointer(PointerDelta { dx: 5000, dy: 0 }),
    );
    pair.advance(20);
    assert_eq!(
        pair.server.phase(),
        Phase::Ready,
        "o controle volta pela direita do cliente"
    );
}

#[test]
fn changing_the_edge_while_controlling_the_client_hands_control_back_first() {
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
    assert_eq!(pair.client.phase(), Phase::Engaged);
    pair.clear_log();

    pair.feed(Side::Server, Input::SetPeerEdge(Edge::Left));

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
            matches!(command, Command::Notify(Notice::EdgeChanged { .. }))
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
fn the_client_does_not_choose_the_edge() {
    let mut pair = connected();

    pair.feed(Side::Client, Input::SetPeerEdge(Edge::Top));

    assert_eq!(
        pair.client.peer_edge(),
        Edge::Left,
        "quem decide é o servidor"
    );
    assert!(edges_adopted(&pair, Side::Client).is_empty());
}

#[test]
fn the_server_does_not_take_an_edge_from_the_client() {
    // Proteção: um cliente de outra versão, ou mal-intencionado, não muda por onde o teclado sai.
    let mut pair = connected();
    pair.advance(210);
    let epoch = pair
        .commands(Side::Client)
        .into_iter()
        .find_map(|command| match command {
            Command::Send { frame, .. } => Some(frame.epoch),
            _ => None,
        })
        .expect("o heartbeat do cliente mostra a época da sessão");
    let seq = pair.client.next_sequence(ChannelId::Control);
    let forged = Frame::new(
        Message::Control(Control::EdgeConfig {
            peer_edge: Edge::Top,
        }),
        seq,
    )
    .in_epoch(epoch);

    pair.feed(
        Side::Server,
        Input::Received {
            carrier: Carrier::Udp,
            frame: forged,
        },
    );

    assert_eq!(pair.server.peer_edge(), Edge::Right);
    assert!(edges_adopted(&pair, Side::Server).is_empty());
}
