//! A meta de atraso da rede (`docs/01-visao-e-escopo.md` §6: mediana ≤ 8 ms, p99 ≤ 25 ms), medida
//! no que é deste crate: o enlace cifrado inteiro — Noise, janela de repetição, socket —, ida e
//! volta, em loopback.
//!
//! Loopback não mede o Wi-Fi: mede o que o produto acrescenta por cima dele. Se isto passar da
//! meta, nenhum Wi-Fi vai salvar; a medida no ar continua sendo da bancada.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::sync::Arc;
use std::time::{Duration, Instant};

use ir_crypto::Identity;
use ir_net::{ConnectMode, Endpoint, EndpointHandle, NetCommand, NetEvent, bind};
use tokio::sync::mpsc::UnboundedReceiver;

async fn proximo(eventos: &mut UnboundedReceiver<NetEvent>) -> NetEvent {
    tokio::time::timeout(Duration::from_secs(10), eventos.recv())
        .await
        .expect("o evento chega")
        .expect("o endpoint está vivo")
}

/// Dois endpoints em loopback, pareados e com o enlace de pé.
async fn pareados() -> (EndpointHandle, EndpointHandle) {
    let socket_de_bob = bind("127.0.0.1:0".parse().unwrap()).await.unwrap();
    let bob_addr = socket_de_bob.local_addr().unwrap();
    let mut bob = Endpoint::spawn(socket_de_bob, Arc::new(Identity::generate()));
    let mut alice = Endpoint::spawn(
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
    assert!(matches!(
        proximo(&mut alice.events).await,
        NetEvent::PairingCode { .. }
    ));
    assert!(matches!(
        proximo(&mut bob.events).await,
        NetEvent::PairingCode { .. }
    ));
    alice
        .commands
        .send(NetCommand::ConfirmPairing(true))
        .unwrap();
    bob.commands.send(NetCommand::ConfirmPairing(true)).unwrap();
    assert!(matches!(
        proximo(&mut alice.events).await,
        NetEvent::Established { .. }
    ));
    assert!(matches!(
        proximo(&mut bob.events).await,
        NetEvent::Established { .. }
    ));
    (alice, bob)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_ida_e_volta_pelo_enlace_cifrado_fica_dentro_da_meta_da_rede() {
    let (mut alice, mut bob) = pareados().await;

    // Bob devolve tudo o que chega, como o `Pong` devolve o `Ping`.
    let comandos_de_bob = bob.commands.clone();
    tokio::spawn(async move {
        while let Some(evento) = bob.events.recv().await {
            if let NetEvent::Frame(quadro) = evento {
                let _ = comandos_de_bob.send(NetCommand::SendFrame(quadro));
            }
        }
    });

    let mut voltas = Vec::new();
    for amostra in 0u32..300 {
        let quadro = amostra.to_le_bytes().to_vec();
        let inicio = Instant::now();
        alice
            .commands
            .send(NetCommand::SendFrame(quadro.clone()))
            .unwrap();
        loop {
            if let NetEvent::Frame(volta) = proximo(&mut alice.events).await
                && volta == quadro
            {
                break;
            }
        }
        voltas.push(inicio.elapsed());
    }
    voltas.sort_unstable();
    let mediana = voltas[voltas.len() / 2];
    let p99 = voltas[voltas.len() * 99 / 100];
    println!("ida e volta em loopback: mediana {mediana:?}, p99 {p99:?}");
    assert!(mediana <= Duration::from_millis(8), "mediana {mediana:?}");
    assert!(p99 <= Duration::from_millis(25), "p99 {p99:?}");
}
