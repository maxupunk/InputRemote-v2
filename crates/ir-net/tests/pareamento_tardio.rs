//! O pareamento que espera o outro lado, em vez de desistir no primeiro datagrama.
//!
//! O defeito: o iniciador mandava a primeira mensagem do handshake uma vez, esperava 1,5 s e
//! desistia em silêncio. Um datagrama perdido — ou o outro serviço ainda subindo — era um pareamento
//! perdido, com a janela dizendo "Aguardando o outro computador" para sempre.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};

use ir_crypto::Identity;
use ir_net::handshake::drive_initiator;
use ir_net::{ConnectMode, Endpoint, NetCommand, NetError, NetEvent, bind};

/// Uma porta livre agora, e que ninguém vai ocupar até o teste pedir.
fn porta_livre() -> SocketAddr {
    let socket = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
    socket.local_addr().unwrap()
}

#[tokio::test]
async fn o_outro_lado_que_sobe_depois_ainda_recebe_o_codigo() {
    let alice_id = Arc::new(Identity::generate());
    let alice = Endpoint::spawn(
        bind("127.0.0.1:0".parse().unwrap()).await.unwrap(),
        Arc::clone(&alice_id),
    );
    let destino = porta_livre();
    alice
        .commands
        .send(NetCommand::Connect {
            peer: destino,
            mode: ConnectMode::Pair,
        })
        .unwrap();

    // Três segundos sem ninguém escutando: o primeiro envio se perde de verdade, e o segundo também.
    tokio::time::sleep(Duration::from_secs(3)).await;
    let mut bob = Endpoint::spawn(bind(destino).await.unwrap(), Arc::new(Identity::generate()));

    let chegou = tokio::time::timeout(Duration::from_secs(10), bob.events.recv())
        .await
        .expect("o código não chegou a quem subiu depois")
        .expect("canal fechado");
    assert!(
        matches!(chegou, NetEvent::PairingCode { .. }),
        "esperava o código, veio {chegou:?}"
    );
}

#[tokio::test]
async fn sem_ninguem_do_outro_lado_a_desistencia_vem_no_prazo_e_nao_na_hora() {
    let socket = bind("127.0.0.1:0".parse().unwrap()).await.unwrap();
    let identidade = Identity::generate();
    let inicio = Instant::now();
    let resultado = drive_initiator(&socket, porta_livre(), &identidade, ConnectMode::Pair).await;
    let levou = inicio.elapsed();
    assert!(
        matches!(resultado, Err(NetError::HandshakeTimeout)),
        "{:?}",
        resultado.err()
    );
    assert!(
        levou >= Duration::from_secs(10),
        "desistiu cedo demais: {levou:?}"
    );
    assert!(levou < Duration::from_secs(15), "esperou demais: {levou:?}");
}
