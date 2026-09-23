//! Os dois papéis combinados (protocolo 5): numa colisão, quem escolheu por último fica, e o outro
//! cede.
#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

mod common;

use common::{Pair, Side, layout};
use ir_proto::carrier::Carrier;
use ir_proto::screens::Edge;
use ir_session::event::Notice;
use ir_session::{Command, Role, SessionConfig};

fn server_at(chosen_at: u64) -> SessionConfig {
    let mut config = SessionConfig::server(Edge::Right);
    config.role_chosen_at = chosen_at;
    config
}

fn client_at(chosen_at: u64) -> SessionConfig {
    let mut config = SessionConfig::client(Edge::Left);
    config.role_chosen_at = chosen_at;
    config
}

fn pair(a: SessionConfig, b: SessionConfig) -> Pair {
    let mut pair = Pair::with_configs(a, b, layout(1920, 1080), layout(1920, 1080));
    pair.connect(Carrier::Udp);
    pair
}

/// O papel que cada posição da bancada foi mandada adotar, se foi.
fn adopted(pair: &Pair, side: Side) -> Vec<(Role, u64)> {
    let mut found = Vec::new();
    for role in [Role::Server, Role::Client] {
        for at in [0, 100, 200] {
            let hit = pair.any(side, |c| {
                matches!(c, Command::Notify(Notice::AdoptRole { role: r, chosen_at })
                    if *r == role && *chosen_at == at)
            });
            if hit {
                found.push((role, at));
            }
        }
    }
    found
}

#[test]
fn two_keyboards_the_older_choice_becomes_controlled() {
    // A escolheu "tem o teclado" em 100; B escolheu o mesmo depois, em 200.
    let pair = pair(server_at(100), server_at(200));
    assert_eq!(adopted(&pair, Side::Server), vec![(Role::Client, 200)]);
    assert!(
        adopted(&pair, Side::Client).is_empty(),
        "quem escolheu por último fica"
    );
}

#[test]
fn two_controlled_the_older_choice_takes_the_keyboard() {
    // O caso relatado: um lado passou a "é controlado" e o outro ainda estava assim.
    let pair = pair(client_at(200), client_at(100));
    assert_eq!(adopted(&pair, Side::Client), vec![(Role::Server, 200)]);
    assert!(adopted(&pair, Side::Server).is_empty());
}

#[test]
fn never_chosen_on_either_side_exactly_one_yields() {
    let pair = pair(server_at(0), server_at(0));
    let yielded = adopted(&pair, Side::Server).len() + adopted(&pair, Side::Client).len();
    assert_eq!(yielded, 1, "o identificador desempata");
}

#[test]
fn roles_that_already_match_change_nothing() {
    let pair = pair(server_at(100), client_at(200));
    assert!(adopted(&pair, Side::Server).is_empty());
    assert!(adopted(&pair, Side::Client).is_empty());
}
