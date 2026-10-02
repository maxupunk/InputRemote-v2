#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use ir_proto::message::{CancelReason, ManifestItem, TransferId};

use super::*;

/// Duas portas em loopback, já com as identidades fixadas uma na outra.
async fn ligadas() -> (EnlaceDeDados, EnlaceDeDados) {
    let aqui = Arc::new(Identity::generate());
    let la = Arc::new(Identity::generate());
    let (chave_daqui, chave_de_la) = (aqui.public(), la.public());

    // Porta efêmera: o teste não pode brigar com a 52525 de um serviço instalado.
    let porta_de_la = Porta::abrir(0, Arc::clone(&la)).await.expect("escuta");
    let alvo = porta_de_la.endereco().expect("endereço");
    let alvo = SocketAddr::from(([127, 0, 0, 1], alvo.port()));

    let atende = tokio::spawn(async move { porta_de_la.aceitar(chave_daqui).await });
    let porta_daqui = Porta::abrir(0, aqui).await.expect("escuta");
    let discado = porta_daqui.discar(alvo, chave_de_la).await.expect("disca");
    let atendido = atende.await.expect("tarefa").expect("atende");
    (discado, atendido)
}

#[tokio::test]
async fn uma_mensagem_atravessa_o_socket_de_verdade() {
    let (mut daqui, mut de_la) = ligadas().await;
    let manifesto = BulkMessage::Manifest {
        id: TransferId(7),
        items: vec![ManifestItem {
            path: "relat\u{f3}rio/a.pdf".to_owned(),
            size: 42,
            is_dir: false,
        }],
        total_bytes: 42,
    };
    daqui
        .remetente
        .enviar_agora(manifesto.clone())
        .await
        .unwrap();
    assert_eq!(de_la.destinatario.receber().await.unwrap(), manifesto);
}

#[tokio::test]
async fn as_duas_direcoes_funcionam_ao_mesmo_tempo() {
    // O padrão real: um lado despeja blocos enquanto o outro confirma.
    let (mut daqui, mut de_la) = ligadas().await;
    let id = TransferId(1);

    let despeja = tokio::spawn(async move {
        for n in 0..16u32 {
            daqui
                .remetente
                .enviar(BulkMessage::FileBlock {
                    id,
                    item: 0,
                    offset: u64::from(n) * 1024,
                    data: vec![u8::try_from(n).unwrap_or(0); 1024],
                })
                .await
                .unwrap();
        }
        // Lê as confirmações que voltaram.
        let mut confirmadas = 0;
        while confirmadas < 16 {
            let voltou = daqui.destinatario.receber().await.unwrap();
            confirmadas += usize::from(matches!(voltou, BulkMessage::Verified { .. }));
        }
        confirmadas
    });

    for n in 0..16u32 {
        match de_la.destinatario.receber().await.unwrap() {
            BulkMessage::FileBlock { offset, data, .. } => {
                assert_eq!(offset, u64::from(n) * 1024);
                assert_eq!(data.len(), 1024);
            }
            outra => panic!("esperava um bloco, veio {outra:?}"),
        }
        de_la
            .remetente
            .enviar_agora(BulkMessage::Verified {
                id,
                item: 0,
                ok: true,
            })
            .await
            .unwrap();
    }
    assert_eq!(despeja.await.unwrap(), 16);
}

#[tokio::test]
async fn quem_atende_recusa_uma_identidade_que_nao_e_a_fixada() {
    // A porta do canal de dados é alcançável por qualquer um na rede local. O que impede um
    // estranho de abrir uma transferência é esta conferência, e nada mais.
    let la = Arc::new(Identity::generate());
    let estranho = Arc::new(Identity::generate());
    let fixada = Identity::generate().public();
    let chave_de_la = la.public();

    let porta_de_la = Porta::abrir(0, Arc::clone(&la)).await.expect("escuta");
    let alvo = porta_de_la.endereco().expect("endereço");
    let alvo = SocketAddr::from(([127, 0, 0, 1], alvo.port()));

    let atende = tokio::spawn(async move { porta_de_la.aceitar(fixada).await });
    let porta_do_estranho = Porta::abrir(0, estranho).await.expect("escuta");
    let _ = porta_do_estranho.discar(alvo, chave_de_la).await;
    assert!(
        atende.await.expect("tarefa").is_err(),
        "um estranho não pode abrir o canal de dados"
    );
}

#[tokio::test]
async fn um_quadro_fora_do_canal_de_dados_e_recusado() {
    // O enlace carrega o canal 5 e nada mais. Aceitar controle por aqui seria um segundo
    // caminho para a máquina de estados da sessão, que é exatamente o que o ADR-0010 separou.
    let (mut daqui, mut de_la) = ligadas().await;
    let quadro = Frame::new(
        Message::Control(ir_proto::message::Control::AckOnly),
        Sequence(0),
    );
    // Codificado como UDP, porque o codec recusaria controle... não: controle viaja em
    // qualquer portador. É o `receber` que tem de barrar.
    let bytes = codec::encode(&quadro, Carrier::Tcp).expect("controle cabe no TCP");
    daqui.remetente.saida.send_now(&bytes).await.unwrap();
    assert!(de_la.destinatario.receber().await.is_err());
}

#[tokio::test]
async fn o_cancelamento_atravessa_como_qualquer_outra_mensagem() {
    let (mut daqui, mut de_la) = ligadas().await;
    let cancel = BulkMessage::Cancel {
        id: TransferId(3),
        reason: CancelReason::UserRequested,
    };
    daqui.remetente.enviar_agora(cancel.clone()).await.unwrap();
    assert_eq!(de_la.destinatario.receber().await.unwrap(), cancel);
}

#[tokio::test]
async fn uma_mensagem_que_nao_cabe_nao_derruba_o_enlace() {
    // O defeito do log 55: o manifesto grande demais era tomado por queda do enlace.
    let (mut daqui, mut de_la) = ligadas().await;
    let gorda = BulkMessage::FileBlock {
        id: TransferId(4),
        item: 0,
        offset: 0,
        data: vec![0; ir_proto::limits::MAX_TCP_PLAINTEXT],
    };
    let falha = daqui.remetente.enviar_agora(gorda).await.unwrap_err();
    assert!(
        matches!(falha, FalhaDeEnvio::NaoCabe(_)),
        "é defeito daqui, não da rede: {falha}"
    );
    assert!(!falha.derrubou_o_enlace());

    let seguinte = BulkMessage::Accept { id: TransferId(5) };
    daqui
        .remetente
        .enviar_agora(seguinte.clone())
        .await
        .unwrap();
    assert_eq!(de_la.destinatario.receber().await.unwrap(), seguinte);
}

#[test]
fn a_regra_da_colisao_chega_ao_servico_sem_ele_conhecer_o_ir_net() {
    let maior = PublicKey([9; 32]);
    let menor = PublicKey([1; 32]);
    assert!(ficar_com_o_proprio(maior, menor));
    assert!(!ficar_com_o_proprio(menor, maior));
}
