#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::sync::Arc;

use ir_crypto::enlace::{Confirmacao, ConnectMode, concluir};
use ir_crypto::turno::Rodadas;
use ir_crypto::{Identity, Transport};
use tokio::sync::mpsc;

use super::{Endpoint, NetEvent, State};
use crate::link::SecureLink;

/// Um transporte de verdade, de um pareamento feito em memória.
fn transporte() -> Transport {
    let (a, b) = (Identity::generate(), Identity::generate());
    let mut ia = ConnectMode::Pair.iniciar(&a).unwrap();
    let mut ib = ConnectMode::Pair.modo().responder(&b).unwrap();
    while !(ia.is_finished() && ib.is_finished()) {
        if ia.is_my_turn() {
            ib.read_message(&ia.write_message().unwrap()).unwrap();
        } else {
            ia.read_message(&ib.write_message().unwrap()).unwrap();
        }
    }
    concluir(ia).unwrap().transport
}

/// Um endpoint esperando a confirmação, com um enlace cujo envio **sempre** falha: o socket é IPv4
/// e o par está num endereço IPv6, que o sistema recusa na hora de mandar.
async fn aguardando_com_envio_quebrado() -> (Endpoint, mpsc::UnboundedReceiver<NetEvent>) {
    let socket = crate::bind("127.0.0.1:0".parse().unwrap()).await.unwrap();
    let link = SecureLink::new(
        Arc::clone(&socket),
        "[::1]:9".parse().unwrap(),
        transporte(),
    );
    let (events, recebidos) = mpsc::unbounded_channel();
    let endpoint = Endpoint {
        socket,
        identity: Arc::new(Identity::generate()),
        events,
        state: State::AwaitingConfirm {
            link,
            peer_static: Identity::generate().public(),
            confirmacao: Confirmacao::default(),
        },
        rodadas: Rodadas::default(),
        aceitar_pareamento: true,
        rechave_tentada: None,
    };
    (endpoint, recebidos)
}

#[tokio::test]
async fn a_confirmacao_que_nao_sai_e_contada_e_nao_engolida() {
    // O rádio sempre relatou; a rede engolia (`let _ =`). O par ficava esperando uma confirmação
    // perdida, e daqui ninguém sabia por quê.
    let (mut endpoint, mut eventos) = aguardando_com_envio_quebrado().await;
    endpoint.confirm_pairing(true).await;
    assert!(
        matches!(eventos.try_recv(), Ok(NetEvent::Error(_))),
        "a falha ao mandar a confirmação precisa chegar ao serviço"
    );
    assert!(
        matches!(endpoint.state, State::AwaitingConfirm { .. }),
        "e o enlace segue esperando o par"
    );
}

#[tokio::test]
async fn a_recusa_que_nao_sai_e_contada_e_o_enlace_cai_mesmo_assim() {
    let (mut endpoint, mut eventos) = aguardando_com_envio_quebrado().await;
    endpoint.confirm_pairing(false).await;
    assert!(matches!(eventos.try_recv(), Ok(NetEvent::Error(_))));
    assert!(matches!(
        eventos.try_recv(),
        Ok(NetEvent::LinkDown("códigos diferentes"))
    ));
    assert!(matches!(endpoint.state, State::Idle));
}
