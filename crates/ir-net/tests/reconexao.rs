//! Reconexão por UDP entre dois endpoints de verdade, em loopback, com as chaves já fixadas.
//!
//! Os dois defeitos que a bancada mostrou depois de uma queda: um lado preso num enlace que do
//! outro lado já não existia, ignorando o handshake do par; e os dois lados discando juntos, em
//! toda rodada, sem nunca firmar.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use ir_crypto::Identity;
use ir_net::{ConnectMode, Endpoint, EndpointHandle, NetCommand, NetEvent, bind};
use tokio::sync::mpsc::UnboundedReceiver;

async fn proximo(eventos: &mut UnboundedReceiver<NetEvent>, prazo: Duration) -> Option<NetEvent> {
    tokio::time::timeout(prazo, eventos.recv())
        .await
        .ok()
        .flatten()
}

/// Espera até um `Established`, deixando passar erros e quedas no caminho.
async fn ate_estabelecer(eventos: &mut UnboundedReceiver<NetEvent>, prazo: Duration) -> bool {
    let fim = tokio::time::Instant::now() + prazo;
    while let Some(evento) = proximo(eventos, fim - tokio::time::Instant::now()).await {
        if matches!(evento, NetEvent::Established { .. }) {
            return true;
        }
    }
    false
}

struct Ponta {
    id: Arc<Identity>,
    endereco: SocketAddr,
    alca: EndpointHandle,
}

async fn subir(id: Arc<Identity>, endereco: &str) -> Ponta {
    let socket = bind(endereco.parse().unwrap()).await.unwrap();
    let endereco = socket.local_addr().unwrap();
    let alca = Endpoint::spawn(socket, Arc::clone(&id));
    Ponta { id, endereco, alca }
}

fn discar(de: &Ponta, para: &Ponta) {
    de.alca
        .commands
        .send(NetCommand::Connect {
            peer: para.endereco,
            mode: ConnectMode::Reconnect(para.id.public()),
        })
        .unwrap();
}

/// Duas pontas com enlace estabelecido.
async fn ligadas() -> (Ponta, Ponta) {
    let mut alice = subir(Arc::new(Identity::generate()), "127.0.0.1:0").await;
    let mut bob = subir(Arc::new(Identity::generate()), "127.0.0.1:0").await;
    discar(&alice, &bob);
    assert!(ate_estabelecer(&mut alice.alca.events, Duration::from_secs(3)).await);
    assert!(ate_estabelecer(&mut bob.alca.events, Duration::from_secs(3)).await);
    (alice, bob)
}

/// Encerra uma ponta e devolve o endereço dela livre para outra subir no lugar.
async fn derrubar(ponta: Ponta) -> SocketAddr {
    ponta.alca.commands.send(NetCommand::Shutdown).unwrap();
    // A tarefa solta o socket ao sair; um instante para isso acontecer.
    tokio::time::sleep(Duration::from_millis(100)).await;
    ponta.endereco
}

#[tokio::test]
async fn um_par_que_reiniciou_reconecta_mesmo_com_o_enlace_velho_de_pe_aqui() {
    let (alice, mut bob) = ligadas().await;
    let id = Arc::clone(&alice.id);
    let endereco = derrubar(alice).await;

    // Alice volta, no mesmo endereço e com a mesma identidade. Bob ainda acha que o enlace velho vale.
    let mut alice = subir(id, &endereco.to_string()).await;
    discar(&alice, &bob);
    assert!(
        ate_estabelecer(&mut alice.alca.events, Duration::from_secs(3)).await,
        "Alice não reconectou"
    );
    assert!(matches!(
        proximo(&mut bob.alca.events, Duration::from_secs(3)).await,
        Some(NetEvent::LinkDown(_))
    ));
    assert!(ate_estabelecer(&mut bob.alca.events, Duration::from_secs(3)).await);

    let quadro = b"depois do reinicio".to_vec();
    alice
        .alca
        .commands
        .send(NetCommand::SendFrame(quadro.clone()))
        .unwrap();
    match proximo(&mut bob.alca.events, Duration::from_secs(3)).await {
        Some(NetEvent::Frame(recebido)) => assert_eq!(recebido, quadro),
        outro => panic!("Bob esperava o quadro, veio {outro:?}"),
    }
}

#[tokio::test]
async fn um_handshake_de_outra_identidade_nao_derruba_o_enlace() {
    let (alice, mut bob) = ligadas().await;
    let endereco = derrubar(alice).await;

    // Do endereço de Alice, mas com outra chave: quem forja o endereço não tem a identidade.
    let intruso = subir(Arc::new(Identity::generate()), &endereco.to_string()).await;
    discar(&intruso, &bob);
    let evento = proximo(&mut bob.alca.events, Duration::from_secs(2)).await;
    assert!(
        evento.is_none(),
        "o enlace de Bob reagiu a um intruso: {evento:?}"
    );
}

#[tokio::test]
async fn discando_os_dois_juntos_em_toda_rodada_o_enlace_firma() {
    let mut alice = subir(Arc::new(Identity::generate()), "127.0.0.1:0").await;
    let mut bob = subir(Arc::new(Identity::generate()), "127.0.0.1:0").await;
    let mut firmou = (false, false);
    // Rodadas como as do serviço, os dois ao mesmo tempo, até firmar. Cada rodada espera o
    // handshake terminar ou desistir, como a rodada de 3 s do serviço faria.
    for _ in 0..6 {
        discar(&alice, &bob);
        discar(&bob, &alice);
        let prazo = Duration::from_millis(1_800);
        let (a, b) = tokio::join!(
            ate_estabelecer(&mut alice.alca.events, prazo),
            ate_estabelecer(&mut bob.alca.events, prazo)
        );
        firmou = (firmou.0 || a, firmou.1 || b);
        if firmou == (true, true) {
            return;
        }
    }
    panic!("as duas pontas discando juntas nunca firmaram: {firmou:?}");
}
