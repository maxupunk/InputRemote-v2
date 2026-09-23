//! O pareamento inteiro, provado sem rádio e sem dois computadores.
//!
//! Dois endpoints de verdade, ligados por um rádio de mentira
//! ([`mentira`](crate::radio::mentira)). É o que a fronteira [`Radio`](crate::radio::Radio)
//! compra: o caminho que no v1 só podia ser testado à mão, com hardware, aqui roda em
//! milissegundos no CI.

use core::time::Duration;
use std::sync::Arc;

use ir_crypto::Identity;
use tokio::sync::mpsc;

use super::{BtCommand, BtEvent, Endpoint, EndpointHandle};
use crate::handshake::ConnectMode;
use crate::radio::mentira::{LA, RadioDeMentira};

/// Quanto se espera por um evento que **deve** chegar.
const PRAZO: Duration = Duration::from_secs(5);

/// Quanto se espera para concluir que um evento **não** vem.
///
/// Curto de propósito: o caminho todo é em memória, então o que não chegou neste tempo não é
/// lentidão.
const SILENCIO: Duration = Duration::from_millis(250);

/// Os dois lados, e as identidades de cada um.
struct Dupla {
    aqui: EndpointHandle,
    la: EndpointHandle,
    /// A chave pública do lado de lá, para a reconexão fixada.
    chave_de_la: ir_crypto::PublicKey,
}

/// Dois endpoints ligados um no outro, pareados no sistema.
fn subir() -> Dupla {
    subir_com(RadioDeMentira::par())
}

fn subir_com(radios: (Arc<RadioDeMentira>, Arc<RadioDeMentira>)) -> Dupla {
    let (radio_aqui, radio_la) = radios;
    let id_aqui = Arc::new(Identity::generate());
    let id_la = Arc::new(Identity::generate());
    let chave_de_la = id_la.public();
    Dupla {
        aqui: Endpoint::spawn(radio_aqui, id_aqui),
        la: Endpoint::spawn(radio_la, id_la),
        chave_de_la,
    }
}

/// O próximo evento, ou falha se ele não vier a tempo.
async fn proximo(eventos: &mut mpsc::UnboundedReceiver<BtEvent>) -> BtEvent {
    tokio::time::timeout(PRAZO, eventos.recv())
        .await
        .expect("o evento precisa chegar a tempo")
        .expect("o endpoint precisa estar vivo")
}

/// O que chegou durante o silêncio, se chegou algo.
async fn durante_o_silencio(eventos: &mut mpsc::UnboundedReceiver<BtEvent>) -> Option<BtEvent> {
    tokio::time::timeout(SILENCIO, eventos.recv())
        .await
        .ok()
        .flatten()
}

fn mandar(alca: &EndpointHandle, comando: BtCommand) {
    alca.commands.send(comando).expect("o endpoint está vivo");
}

/// Começa um pareamento e devolve os dois códigos, na ordem: aqui, lá.
async fn parear(dupla: &mut Dupla) -> ([u8; 6], [u8; 6]) {
    mandar(
        &dupla.aqui,
        BtCommand::Connect {
            peer: LA,
            mode: ConnectMode::Pair,
        },
    );
    let codigo_aqui = esperar_codigo(&mut dupla.aqui.events).await;
    let codigo_la = esperar_codigo(&mut dupla.la.events).await;
    (codigo_aqui, codigo_la)
}

async fn esperar_codigo(eventos: &mut mpsc::UnboundedReceiver<BtEvent>) -> [u8; 6] {
    match proximo(eventos).await {
        BtEvent::PairingCode { code, .. } => code,
        outro => panic!("esperava um código de pareamento, veio {outro:?}"),
    }
}

#[tokio::test]
async fn as_duas_telas_mostram_o_mesmo_codigo() {
    // O código é o que duas pessoas comparam em voz alta. Se os dois lados derivarem números
    // diferentes de um handshake legítimo, o produto acusa ataque onde não há.
    let mut dupla = subir();
    let (aqui, la) = parear(&mut dupla).await;
    assert_eq!(aqui, la);
    assert!(aqui.iter().all(|digito| *digito <= 9), "{aqui:?}");
}

#[tokio::test]
async fn nada_de_sessao_trafega_antes_das_duas_confirmacoes() {
    // A garantia de docs/04 §3.2, e a razão de este módulo existir. Um quadro que passasse aqui
    // teria atravessado um enlace que ninguém ainda confirmou ser com quem diz ser.
    let mut dupla = subir();
    let _ = parear(&mut dupla).await;

    mandar(&dupla.aqui, BtCommand::quadro(b"tecla".to_vec()));
    assert!(
        durante_o_silencio(&mut dupla.la.events).await.is_none(),
        "nenhum quadro pode atravessar antes das confirmações"
    );

    // Um lado só confirmando também não basta — e este é o caso que mais fácil se implementa
    // errado, porque parece pronto.
    mandar(&dupla.aqui, BtCommand::ConfirmPairing(true));
    assert!(
        durante_o_silencio(&mut dupla.aqui.events).await.is_none(),
        "uma confirmação só não estabelece"
    );
    assert!(
        durante_o_silencio(&mut dupla.la.events).await.is_none(),
        "o outro lado ainda não disse nada"
    );
}

#[tokio::test]
async fn com_as_duas_confirmacoes_a_sessao_estabelece_e_o_quadro_passa() {
    let mut dupla = subir();
    let _ = parear(&mut dupla).await;

    mandar(&dupla.aqui, BtCommand::ConfirmPairing(true));
    mandar(&dupla.la, BtCommand::ConfirmPairing(true));

    for lado in [&mut dupla.aqui, &mut dupla.la] {
        match proximo(&mut lado.events).await {
            BtEvent::Established { .. } => {}
            outro => panic!("esperava o enlace pronto, veio {outro:?}"),
        }
    }

    mandar(&dupla.aqui, BtCommand::quadro(b"tecla".to_vec()));
    match proximo(&mut dupla.la.events).await {
        BtEvent::Frame(bytes) => assert_eq!(bytes, b"tecla"),
        outro => panic!("esperava o quadro, veio {outro:?}"),
    }
}

#[tokio::test]
async fn um_quadro_que_esperou_demais_na_fila_nao_vale_o_radio() {
    // O rádio que travou sob interferência despejaria, ao voltar, segundos de quadros velhos na
    // frente dos novos. O velho é descartado antes de ser cifrado; o novo passa.
    let mut dupla = subir();
    let _ = parear(&mut dupla).await;
    mandar(&dupla.aqui, BtCommand::ConfirmPairing(true));
    mandar(&dupla.la, BtCommand::ConfirmPairing(true));
    for lado in [&mut dupla.aqui, &mut dupla.la] {
        assert!(matches!(
            proximo(&mut lado.events).await,
            BtEvent::Established { .. }
        ));
    }

    let ha_um_segundo = tokio::time::Instant::now()
        .checked_sub(Duration::from_secs(1))
        .expect("o relógio já andou um segundo");
    mandar(
        &dupla.aqui,
        BtCommand::SendFrame {
            bytes: b"velho".to_vec(),
            queued_at: ha_um_segundo,
        },
    );
    mandar(&dupla.aqui, BtCommand::quadro(b"novo".to_vec()));

    match proximo(&mut dupla.la.events).await {
        BtEvent::Frame(bytes) => assert_eq!(bytes, b"novo", "o velho não pode passar"),
        outro => panic!("esperava o quadro novo, veio {outro:?}"),
    }
    assert!(
        durante_o_silencio(&mut dupla.la.events).await.is_none(),
        "e o enlace continua de pé, sem mais nada"
    );
}

#[tokio::test]
async fn cada_lado_guarda_a_chave_estatica_do_outro() {
    // É o que será fixado, e é o que faz a reconexão recusar qualquer outra máquina depois.
    let mut dupla = subir();
    mandar(
        &dupla.aqui,
        BtCommand::Connect {
            peer: LA,
            mode: ConnectMode::Pair,
        },
    );
    let chave_vista = match proximo(&mut dupla.aqui.events).await {
        BtEvent::PairingCode { peer_static, .. } => peer_static,
        outro => panic!("esperava o código, veio {outro:?}"),
    };
    assert_eq!(chave_vista, dupla.chave_de_la);
}

#[tokio::test]
async fn a_reconexao_com_a_chave_fixada_nao_pede_confirmacao() {
    // Pedir o código de novo a cada reconexão treinaria o usuário a clicar sem olhar — o que
    // esvazia a única defesa que depende dele.
    let mut dupla = subir();
    mandar(
        &dupla.aqui,
        BtCommand::Connect {
            peer: LA,
            mode: ConnectMode::Reconnect(dupla.chave_de_la),
        },
    );
    for lado in [&mut dupla.aqui, &mut dupla.la] {
        match proximo(&mut lado.events).await {
            BtEvent::Established { .. } => {}
            outro => panic!("a reconexão estabelece direto, veio {outro:?}"),
        }
    }
}

#[tokio::test]
async fn recusar_o_codigo_derruba_o_enlace_nos_dois_lados() {
    // Quem disse "são diferentes" viu algo errado. O outro lado precisa sair da tela de
    // comparação também, em vez de ficar esperando o próprio prazo vencer.
    let mut dupla = subir();
    let _ = parear(&mut dupla).await;

    mandar(&dupla.aqui, BtCommand::ConfirmPairing(false));

    match proximo(&mut dupla.aqui.events).await {
        BtEvent::LinkDown(motivo) => assert_eq!(motivo, "códigos diferentes"),
        outro => panic!("esperava a queda, veio {outro:?}"),
    }
    match proximo(&mut dupla.la.events).await {
        BtEvent::LinkDown(motivo) => assert_eq!(motivo, "o par recusou o pareamento"),
        outro => panic!("esperava a queda do outro lado, veio {outro:?}"),
    }
}

#[tokio::test]
async fn conectar_a_quem_nao_esta_pareado_diz_o_que_fazer() {
    // A exigência escrita no ADR-0005: distinguir "não pareado no sistema" de "pareado, mas o
    // serviço não responde" — e dizer qual é. Sem isso o usuário mexe no lugar errado.
    let mut dupla = subir_com(RadioDeMentira::sem_pareamento());
    mandar(
        &dupla.aqui,
        BtCommand::Connect {
            peer: LA,
            mode: ConnectMode::Pair,
        },
    );
    match proximo(&mut dupla.aqui.events).await {
        BtEvent::Error(texto) => {
            assert!(texto.contains("não está pareado"), "{texto}");
            assert!(texto.contains("configurações de Bluetooth"), "{texto}");
        }
        outro => panic!("esperava o erro explicado, veio {outro:?}"),
    }
}

#[tokio::test]
async fn o_par_que_vai_embora_e_uma_queda_com_o_motivo_certo() {
    // "O par encerrou o canal" e "o quadro não abriu" são causas diferentes. Registrar uma pela
    // outra esconde justamente o que aconteceu.
    let mut dupla = subir();
    let _ = parear(&mut dupla).await;
    mandar(&dupla.aqui, BtCommand::ConfirmPairing(true));
    mandar(&dupla.la, BtCommand::ConfirmPairing(true));
    for lado in [&mut dupla.aqui, &mut dupla.la] {
        let _ = proximo(&mut lado.events).await;
    }

    // O outro computador desliga o serviço.
    mandar(&dupla.la, BtCommand::Shutdown);

    // A falha produz os dois eventos, e é assim que tem de ser: o erro leva a frase que a tela
    // mostra, a queda leva o motivo curto de que a máquina de estados precisa. Aqui interessa o
    // segundo — mas o primeiro também é conferido, porque um sem o outro seria defeito.
    let mut explicou = false;
    let motivo = loop {
        match proximo(&mut dupla.aqui.events).await {
            BtEvent::LinkDown(motivo) => break motivo,
            BtEvent::Error(texto) => {
                assert!(texto.contains("InputRemote está em execução"), "{texto}");
                explicou = true;
            }
            outro => panic!("esperava a queda, veio {outro:?}"),
        }
    };
    assert_eq!(motivo, "o par encerrou o canal");
    assert!(explicou, "a tela precisa receber a explicação junto");
}

#[tokio::test]
async fn desconectar_a_pedido_derruba_so_uma_vez() {
    let mut dupla = subir();
    let _ = parear(&mut dupla).await;

    mandar(&dupla.aqui, BtCommand::Disconnect);
    match proximo(&mut dupla.aqui.events).await {
        BtEvent::LinkDown(motivo) => assert_eq!(motivo, "pedido local"),
        outro => panic!("esperava a queda, veio {outro:?}"),
    }

    // Já ocioso: um segundo pedido não produz outra queda, senão a interface veria duas.
    mandar(&dupla.aqui, BtCommand::Disconnect);
    assert!(durante_o_silencio(&mut dupla.aqui.events).await.is_none());
}

#[tokio::test]
async fn o_radio_desligado_no_meio_e_contado_como_perdido() {
    let mut alca = Endpoint::spawn(
        RadioDeMentira::desligado_depois(),
        Arc::new(Identity::generate()),
    );
    let evento = proximo(&mut alca.events).await;
    assert!(
        matches!(evento, BtEvent::RadioLost(_)),
        "antes a escuta esperava para sempre, e o rádio não voltava: {evento:?}"
    );
    // O endpoint terminou: não há mais quem ouvir comandos.
    tokio::time::sleep(SILENCIO).await;
    assert!(alca.commands.send(BtCommand::Disconnect).is_err());
}
