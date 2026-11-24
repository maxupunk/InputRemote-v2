#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use ir_crypto::enlace::{ConnectMode, concluir};
use ir_crypto::turno::Rodadas;
use ir_crypto::{Identity, PublicKey};
use tokio::sync::mpsc::{self, UnboundedReceiver};

use super::{IDADE_MAXIMA, Papel, troca_por_idade};
use crate::endpoint::{Endpoint, NetCommand, NetEvent, State};
use crate::link::SecureLink;
use crate::wire::Mode;

async fn proximo(eventos: &mut UnboundedReceiver<NetEvent>) -> NetEvent {
    tokio::time::timeout(Duration::from_secs(10), eventos.recv())
        .await
        .expect("o evento chega")
        .expect("o endpoint está vivo")
}

/// Dois endpoints em loopback, pareados e com o enlace de pé, rodando sozinhos.
async fn pareados() -> (crate::EndpointHandle, crate::EndpointHandle) {
    let socket_de_bob = crate::bind("127.0.0.1:0".parse().unwrap()).await.unwrap();
    let bob_addr = socket_de_bob.local_addr().unwrap();
    let mut bob = Endpoint::spawn(socket_de_bob, Arc::new(Identity::generate()));
    let mut alice = Endpoint::spawn(
        crate::bind("127.0.0.1:0".parse().unwrap()).await.unwrap(),
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

#[tokio::test]
async fn a_troca_de_chaves_nao_derruba_o_enlace() {
    let (mut alice, mut bob) = pareados().await;

    alice.commands.send(NetCommand::ForcarRechave).unwrap();
    alice
        .commands
        .send(NetCommand::SendFrame(b"antes".to_vec()))
        .unwrap();
    assert!(matches!(proximo(&mut bob.events).await, NetEvent::Frame(q) if q == b"antes"));

    // Ida e volta várias vezes: em algum ponto as chaves trocam, e nada se perde nem cai.
    for rodada in 0..5u8 {
        alice
            .commands
            .send(NetCommand::SendFrame(vec![b'a', rodada]))
            .unwrap();
        let chegou = proximo(&mut bob.events).await;
        assert!(
            matches!(&chegou, NetEvent::Frame(q) if *q == [b'a', rodada]),
            "{chegou:?}"
        );
        bob.commands
            .send(NetCommand::SendFrame(vec![b'b', rodada]))
            .unwrap();
        let voltou = proximo(&mut alice.events).await;
        assert!(
            matches!(&voltou, NetEvent::Frame(q) if *q == [b'b', rodada]),
            "{voltou:?}"
        );
    }
}

#[test]
fn por_idade_so_a_menor_chave_pede_e_so_depois_de_dez_minutos() {
    let (menor, maior) = (PublicKey([1; 32]), PublicKey([2; 32]));
    assert!(troca_por_idade(&menor, &maior, IDADE_MAXIMA));
    assert!(
        !troca_por_idade(&maior, &menor, IDADE_MAXIMA),
        "a outra ponta só atende"
    );
    assert!(!troca_por_idade(&menor, &maior, IDADE_MAXIMA / 2));
}

/// Um lado conduzido à mão: o teste decide o que chega, o que se perde e quando.
struct Lado {
    ep: Endpoint,
    eventos: UnboundedReceiver<NetEvent>,
}

impl Lado {
    /// Lê o próximo datagrama que o outro lado mandou para este.
    async fn ler(&self) -> (SocketAddr, Vec<u8>) {
        let mut buf = [0u8; 2048];
        let (len, de) =
            tokio::time::timeout(Duration::from_secs(5), self.ep.socket.recv_from(&mut buf))
                .await
                .expect("o datagrama chega")
                .unwrap();
        (de, buf.get(..len).unwrap().to_vec())
    }

    /// Lê e entrega ao endpoint o próximo datagrama.
    async fn receber(&mut self) {
        let (de, datagrama) = self.ler().await;
        self.ep.on_datagram(de, &datagrama).await;
    }

    async fn enviar(&mut self, quadro: &[u8]) {
        self.ep.send_frame(quadro).await;
    }

    fn quadro(&mut self) -> Vec<u8> {
        match self.eventos.try_recv() {
            Ok(NetEvent::Frame(quadro)) => quadro,
            outro => panic!("esperava um quadro, veio {outro:?}"),
        }
    }

    fn papel_da_reserva(&self) -> Option<Papel> {
        match &self.ep.state {
            State::Established { reserva, .. } => reserva.as_ref().map(|r| r.papel),
            _ => panic!("o enlace caiu"),
        }
    }

    fn forcar_rechave(&mut self) {
        if let State::Established { link, .. } = &mut self.ep.state {
            link.pedir_rechave();
        }
    }
}

/// Dois lados com o enlace de pé por um `Noise_IK` de verdade, sem tarefa nenhuma rodando.
async fn lados() -> (Lado, Lado) {
    let (a, b) = (
        Arc::new(Identity::generate()),
        Arc::new(Identity::generate()),
    );
    let mut ia = ConnectMode::Reconnect(b.public()).iniciar(&a).unwrap();
    let mut ib = Mode::Reconnect.responder(&b).unwrap();
    ib.read_message(&ia.write_message().unwrap()).unwrap();
    ia.read_message(&ib.write_message().unwrap()).unwrap();
    let (ta, tb) = (concluir(ia).unwrap(), concluir(ib).unwrap());
    let sa = crate::bind("127.0.0.1:0".parse().unwrap()).await.unwrap();
    let sb = crate::bind("127.0.0.1:0".parse().unwrap()).await.unwrap();
    let (ea, eb) = (sa.local_addr().unwrap(), sb.local_addr().unwrap());
    let lado = |socket: Arc<tokio::net::UdpSocket>, eu: Arc<Identity>, par, transporte, do_par| {
        let (events, recebidos) = mpsc::unbounded_channel();
        let link = SecureLink::new(Arc::clone(&socket), par, transporte);
        Lado {
            ep: Endpoint {
                socket,
                identity: eu,
                events,
                state: State::Established {
                    link,
                    peer_static: do_par,
                    reserva: None,
                },
                rodadas: Rodadas::default(),
                aceitar_pareamento: false,
                rechave_tentada: None,
                rechave: None,
            },
            eventos: recebidos,
        }
    };
    let alice = lado(sa, Arc::clone(&a), eb, ta.transport, b.public());
    let bob = lado(sb, b, ea, tb.transport, a.public());
    (alice, bob)
}

#[tokio::test]
async fn a_resposta_perdida_nao_desalinha_as_chaves() {
    // O defeito da bancada: quem respondia passava a cifrar com as chaves novas na hora, e a
    // resposta perdida deixava cada lado com um jogo — nenhum quadro abria do outro lado.
    let (mut alice, mut bob) = lados().await;

    alice.forcar_rechave();
    alice.enviar(b"um").await; // o quadro, e o pedido de troca logo atrás
    bob.receber().await;
    assert_eq!(bob.quadro(), b"um");
    bob.receber().await; // o pedido: bob responde
    assert_eq!(bob.papel_da_reserva(), Some(Papel::Proxima));
    let _perdida = alice.ler().await; // a resposta se perde no caminho

    bob.enviar(b"dois").await;
    alice.receber().await;
    assert_eq!(
        alice.quadro(),
        b"dois",
        "bob segue cifrando com as chaves de antes"
    );
    alice.enviar(b"tres").await;
    bob.receber().await;
    assert_eq!(bob.quadro(), b"tres");
}

#[tokio::test]
async fn a_troca_se_confirma_com_o_primeiro_quadro_e_nada_se_perde() {
    let (mut alice, mut bob) = lados().await;

    alice.forcar_rechave();
    alice.enviar(b"um").await;
    bob.receber().await;
    bob.receber().await; // o pedido: bob responde e guarda as próximas
    let _ = bob.quadro();
    bob.enviar(b"cifrado-antes").await; // ainda com as chaves de antes
    alice.receber().await; // a resposta: alice troca
    alice.receber().await; // e o quadro de bob, que ele cifrou antes de saber
    assert_eq!(
        alice.quadro(),
        b"cifrado-antes",
        "as anteriores ainda abrem"
    );
    assert_eq!(alice.papel_da_reserva(), Some(Papel::Anterior));

    alice.enviar(b"novo").await;
    bob.receber().await;
    assert_eq!(
        bob.quadro(),
        b"novo",
        "as próximas de bob abrem o primeiro quadro novo"
    );
    assert_eq!(bob.papel_da_reserva(), Some(Papel::Anterior), "e bob troca");

    bob.enviar(b"volta").await;
    alice.receber().await;
    assert_eq!(alice.quadro(), b"volta");
}

#[tokio::test]
async fn o_pedido_sem_resposta_vence_e_outro_sai_depois() {
    let (mut alice, mut bob) = lados().await;

    alice.forcar_rechave();
    alice.enviar(b"um").await;
    bob.receber().await;
    let _pedido_perdido = bob.ler().await;
    assert!(alice.ep.rechave.is_some());

    // O tempo passa: o pedido vence, e a espera entre tentativas também.
    if let Some(pendente) = alice.ep.rechave.as_mut() {
        pendente.desde = std::time::Instant::now()
            .checked_sub(super::PRAZO_DA_RESPOSTA)
            .unwrap();
    }
    alice.ep.rechave_tentada = None;
    alice.enviar(b"dois").await; // o quadro, e o pedido novo
    bob.receber().await;
    bob.receber().await;
    let _ = (bob.quadro(), bob.quadro());
    alice.receber().await; // a resposta ao pedido novo
    assert_eq!(alice.papel_da_reserva(), Some(Papel::Anterior));
    alice.enviar(b"tres").await;
    bob.receber().await;
    assert_eq!(bob.quadro(), b"tres");
}

#[tokio::test]
async fn quadros_do_par_nao_renovam_o_prazo_do_handshake() {
    // O travamento da bancada: o par mandando quadros de dados a cada 50 ms, e cada um descartado
    // recomeçava a espera de 1,5 s — o handshake nunca vencia, e o endpoint nunca mais enviava.
    let par = crate::bind("127.0.0.1:0".parse().unwrap()).await.unwrap();
    let daqui = crate::bind("127.0.0.1:0".parse().unwrap()).await.unwrap();
    let (de_par, para_daqui) = (par.local_addr().unwrap(), daqui.local_addr().unwrap());
    let ruido = tokio::spawn(async move {
        loop {
            let _ = par.send_to(b"\xffquadro de dados", para_daqui).await;
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    });
    let inicio = std::time::Instant::now();
    let identidade = Identity::generate();
    // O prazo de fora existe para o defeito falhar o teste em vez de travá-lo para sempre.
    let resultado = tokio::time::timeout(
        Duration::from_secs(6),
        crate::handshake::drive_initiator(
            &daqui,
            de_par,
            &identidade,
            ConnectMode::Reconnect(Identity::generate().public()),
        ),
    )
    .await
    .expect("o handshake ficou preso esperando, renovando o prazo a cada quadro do par");
    ruido.abort();
    assert!(resultado.is_err(), "ninguém respondeu ao handshake");
    assert!(
        inicio.elapsed() < Duration::from_secs(4),
        "o prazo venceu apesar do ruído: {:?}",
        inicio.elapsed()
    );
}
