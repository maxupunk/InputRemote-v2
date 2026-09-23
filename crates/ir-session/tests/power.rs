//! A economia de energia do Wi-Fi viajando entre as duas pontas.
//!
//! Quem sente as travadas é quem olha a tela do outro lado: cada ponta conta ao par como está a
//! placa dela, e o par pode pedir que ela pare de cochilar.
#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

mod common;

use common::{Pair, Side};
use ir_proto::carrier::Carrier;
use ir_proto::message::NetworkPowerSaving;
use ir_session::Input;
use ir_session::event::Notice;

#[test]
fn the_peer_learns_the_wifi_power_saving_when_the_session_comes_up() {
    let mut pair = Pair::matched();
    pair.feed(
        Side::Client,
        Input::LocalNetworkPower(NetworkPowerSaving::On),
    );
    pair.connect(Carrier::Udp);

    assert!(
        pair.notices(Side::Server)
            .contains(&Notice::PeerNetworkPower(NetworkPowerSaving::On)),
        "o servidor, que é quem olha a tela, fica sabendo da placa do cliente"
    );
}

#[test]
fn every_reading_is_told_so_a_peer_that_missed_one_catches_up() {
    let mut pair = Pair::matched();
    pair.connect(Carrier::Udp);
    pair.clear_log();

    pair.feed(
        Side::Client,
        Input::LocalNetworkPower(NetworkPowerSaving::OnBattery),
    );
    pair.feed(
        Side::Client,
        Input::LocalNetworkPower(NetworkPowerSaving::OnBattery),
    );

    let told = pair
        .notices(Side::Server)
        .iter()
        .filter(|n| matches!(n, Notice::PeerNetworkPower(_)))
        .count();
    assert_eq!(
        told, 2,
        "cada verificação é repetida ao par: se uma se perdeu, a seguinte corrige"
    );
}

#[test]
fn asking_the_peer_to_stop_saving_power_reaches_it() {
    let mut pair = Pair::matched();
    pair.connect(Carrier::Udp);
    pair.clear_log();

    pair.feed(Side::Server, Input::DisablePeerNetworkPowerSaving);

    assert!(
        pair.notices(Side::Client)
            .contains(&Notice::NetworkPowerFixRequested)
    );
    assert!(
        !pair
            .notices(Side::Server)
            .contains(&Notice::PeerCannotFixNetworkPower)
    );
}

#[test]
fn without_a_session_the_request_says_it_could_not_go() {
    let mut pair = Pair::matched();
    pair.feed(Side::Server, Input::DisablePeerNetworkPowerSaving);

    assert!(
        pair.notices(Side::Server)
            .contains(&Notice::PeerCannotFixNetworkPower)
    );
}
