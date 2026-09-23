//! Com a janela de pareamento fechada, um pedido de fora não põe código na tela nem ocupa o
//! endpoint — e a reconexão do par de verdade continua sendo atendida (log 45).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::sync::Arc;
use std::time::Duration;

use ir_crypto::Identity;
use ir_net::{ConnectMode, Endpoint, NetCommand, NetEvent, bind};

#[tokio::test]
async fn o_pedido_de_fora_e_ignorado_com_a_janela_fechada() {
    let bob_socket = bind("127.0.0.1:0".parse().unwrap()).await.unwrap();
    let bob_addr = bob_socket.local_addr().unwrap();
    let mut bob = Endpoint::spawn(bob_socket, Arc::new(Identity::generate()));
    bob.commands.send(NetCommand::AcceptPairing(false)).unwrap();

    let intruso = Endpoint::spawn(
        bind("127.0.0.1:0".parse().unwrap()).await.unwrap(),
        Arc::new(Identity::generate()),
    );
    intruso
        .commands
        .send(NetCommand::Connect {
            peer: bob_addr,
            mode: ConnectMode::Pair,
        })
        .unwrap();

    // O intruso insiste por uns segundos; nada chega ao serviço de Bob.
    let chegou = tokio::time::timeout(Duration::from_secs(3), bob.events.recv()).await;
    assert!(chegou.is_err(), "nada devia chegar, veio {chegou:?}");
}

#[tokio::test]
async fn a_janela_aberta_volta_a_atender() {
    let bob_socket = bind("127.0.0.1:0".parse().unwrap()).await.unwrap();
    let bob_addr = bob_socket.local_addr().unwrap();
    let mut bob = Endpoint::spawn(bob_socket, Arc::new(Identity::generate()));
    bob.commands.send(NetCommand::AcceptPairing(false)).unwrap();
    bob.commands.send(NetCommand::AcceptPairing(true)).unwrap();

    let alice = Endpoint::spawn(
        bind("127.0.0.1:0".parse().unwrap()).await.unwrap(),
        Arc::new(Identity::generate()),
    );
    alice
        .commands
        .send(NetCommand::Connect {
            peer: bob_addr,
            mode: ConnectMode::Pair,
        })
        .unwrap();

    let chegou = tokio::time::timeout(Duration::from_secs(10), bob.events.recv())
        .await
        .expect("o código não chegou")
        .expect("canal fechado");
    assert!(matches!(chegou, NetEvent::PairingCode { .. }), "{chegou:?}");
}
