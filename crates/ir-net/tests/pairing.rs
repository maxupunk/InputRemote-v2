//! Um pareamento inteiro entre dois endpoints, por UDP em loopback.
//!
//! É o teste que prova cripto + rede juntos, sem hardware e sem segundo computador: dois sockets
//! na mesma máquina, dois endpoints, o handshake, o código igual dos dois lados, a confirmação
//! dupla e um quadro atravessando cifrado.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::sync::Arc;
use std::time::Duration;

use ir_crypto::Identity;
use ir_net::{ConnectMode, Endpoint, EndpointHandle, NetCommand, NetEvent, bind};
use tokio::sync::mpsc::UnboundedReceiver;

/// Espera pelo próximo evento de um endpoint, com prazo.
async fn next_event(events: &mut UnboundedReceiver<NetEvent>) -> NetEvent {
    tokio::time::timeout(Duration::from_secs(3), events.recv())
        .await
        .expect("evento não chegou a tempo")
        .expect("canal de eventos fechado")
}

/// Dois endpoints prontos, com Alice já iniciando o pareamento com Bob.
struct Pair {
    alice: EndpointHandle,
    bob: EndpointHandle,
    alice_id: Arc<Identity>,
    bob_id: Arc<Identity>,
}

async fn start_pairing() -> Pair {
    let alice_id = Arc::new(Identity::generate());
    let bob_id = Arc::new(Identity::generate());
    let alice_sock = bind("127.0.0.1:0".parse().unwrap()).await.unwrap();
    let bob_sock = bind("127.0.0.1:0".parse().unwrap()).await.unwrap();
    let bob_addr = bob_sock.local_addr().unwrap();

    let alice = Endpoint::spawn(alice_sock, Arc::clone(&alice_id));
    let bob = Endpoint::spawn(bob_sock, Arc::clone(&bob_id));
    alice
        .commands
        .send(NetCommand::Connect {
            peer: bob_addr,
            mode: ConnectMode::Pair,
        })
        .unwrap();
    Pair {
        alice,
        bob,
        alice_id,
        bob_id,
    }
}

/// Recebe o código de pareamento de um lado, conferindo a chave que ele aprendeu.
async fn take_code(events: &mut UnboundedReceiver<NetEvent>, expected: &Identity) -> [u8; 6] {
    match next_event(events).await {
        NetEvent::PairingCode {
            code, peer_static, ..
        } => {
            assert_eq!(peer_static, expected.public(), "aprendeu a chave certa");
            code
        }
        other => panic!("esperava código, veio {other:?}"),
    }
}

#[tokio::test]
async fn a_full_pairing_lets_a_frame_cross_encrypted() {
    let mut p = start_pairing().await;

    let alice_code = take_code(&mut p.alice.events, &p.bob_id).await;
    let bob_code = take_code(&mut p.bob.events, &p.alice_id).await;
    assert_eq!(alice_code, bob_code, "sem homem no meio, os códigos batem");

    p.alice
        .commands
        .send(NetCommand::ConfirmPairing(true))
        .unwrap();
    p.bob
        .commands
        .send(NetCommand::ConfirmPairing(true))
        .unwrap();
    assert!(matches!(
        next_event(&mut p.alice.events).await,
        NetEvent::Established { .. }
    ));
    assert!(matches!(
        next_event(&mut p.bob.events).await,
        NetEvent::Established { .. }
    ));

    let frame = b"um quadro de teste".to_vec();
    p.alice
        .commands
        .send(NetCommand::SendFrame(frame.clone()))
        .unwrap();
    match next_event(&mut p.bob.events).await {
        NetEvent::Frame(received) => assert_eq!(received, frame),
        other => panic!("Bob esperava um quadro, veio {other:?}"),
    }
}

#[tokio::test]
async fn a_rejected_pairing_does_not_establish() {
    let mut p = start_pairing().await;
    let _ = take_code(&mut p.alice.events, &p.bob_id).await;
    let _ = take_code(&mut p.bob.events, &p.alice_id).await;

    // Alice diz que os códigos são diferentes: nenhum dos dois estabelece.
    p.alice
        .commands
        .send(NetCommand::ConfirmPairing(false))
        .unwrap();
    assert!(matches!(
        next_event(&mut p.alice.events).await,
        NetEvent::LinkDown(_)
    ));
    assert!(matches!(
        next_event(&mut p.bob.events).await,
        NetEvent::LinkDown(_)
    ));
}
