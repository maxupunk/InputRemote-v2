//! A montagem do canal 4 contra um par que erra ou mente.

use ir_proto::carrier::Carrier;
use ir_proto::channel::ChannelId;
use ir_proto::frame::{Ack, Epoch, Frame, Sequence};

use super::*;

fn oferta_de(area: &mut Area, texto: &str) -> (ClipId, u32, [u8; 32]) {
    match area.oferecer(ClipText::new(texto.to_owned()).unwrap()) {
        ClipboardMessage::Offer { id, size, hash, .. } => (id, size, hash),
        outra => panic!("{outra:?}"),
    }
}

/// Leva um texto de uma área a outra, sem sessão no meio.
fn atravessar(texto: &str) -> Option<ClipText> {
    let (mut origem, mut destino) = (Area::default(), Area::default());
    let (id, size, hash) = oferta_de(&mut origem, texto);
    assert_eq!(
        destino.ofereceram(id, ClipKind::Text, size, hash),
        ClipboardMessage::Request { id }
    );
    origem.pedido(id);
    let mut chegou = None;
    while let Some(mensagem) = origem.fila.pop_front() {
        match mensagem {
            ClipboardMessage::Chunk { id, index, data } => destino.pedaco(id, index, &data),
            ClipboardMessage::Done { id } => chegou = destino.fim(id),
            outra => panic!("{outra:?}"),
        }
    }
    chegou
}

#[test]
fn o_pior_quadro_de_pedaco_cabe_no_menor_portador() {
    // Todos os campos no maior valor que codifica: se este cabe, qualquer pedaço cabe.
    let frame = Frame::new(
        Message::Clipboard(ClipboardMessage::Chunk {
            id: ClipId(u32::MAX),
            index: u32::MAX,
            data: vec![0xff; PEDACO],
        }),
        Sequence(u32::MAX),
    )
    .with_ack(ChannelId::ClipboardText, Ack::new(Sequence(u32::MAX)))
    .in_epoch(Epoch(u32::MAX));
    for carrier in [Carrier::Rfcomm, Carrier::Udp] {
        ir_proto::codec::encode(&frame, carrier).expect("o pedaço não cabe no quadro");
    }
}

#[test]
fn texto_vazio_e_texto_de_um_pedaco_exato_atravessam() {
    assert_eq!(atravessar("").unwrap().as_str(), "");
    let exato = "x".repeat(PEDACO);
    assert_eq!(atravessar(&exato).unwrap().as_str(), exato);
    let acentuado = "ç".repeat(PEDACO); // 2 bytes cada: o corte cai no meio de um caractere
    assert_eq!(atravessar(&acentuado).unwrap().as_str(), acentuado);
}

#[test]
fn pedaco_fora_de_ordem_abandona_a_montagem() {
    let mut area = Area::default();
    let id = ClipId(7);
    area.ofereceram(id, ClipKind::Text, 10, [0; 32]);
    area.pedaco(id, 1, b"abc");
    assert!(area.montagem.is_none());
    assert!(area.fim(id).is_none());
}

#[test]
fn pedaco_alem_do_anunciado_abandona_a_montagem() {
    let mut area = Area::default();
    let id = ClipId(7);
    area.ofereceram(id, ClipKind::Text, 2, [0; 32]);
    area.pedaco(id, 0, b"abc");
    assert!(area.montagem.is_none());
}

#[test]
fn pedaco_de_oferta_antiga_nao_estraga_a_nova() {
    let mut area = Area::default();
    area.ofereceram(ClipId(2), ClipKind::Text, 3, [0; 32]);
    area.pedaco(ClipId(1), 0, b"velho");
    assert!(area.montagem.is_some());
}

#[test]
fn resumo_que_nao_confere_nao_vira_clipboard() {
    let mut area = Area::default();
    let id = ClipId(1);
    area.ofereceram(id, ClipKind::Text, 3, [9; 32]);
    area.pedaco(id, 0, b"abc");
    assert!(area.fim(id).is_none());
}

#[test]
fn bytes_que_nao_sao_utf8_nao_viram_clipboard() {
    let mut area = Area::default();
    let id = ClipId(1);
    let bytes = [0xff, 0xfe];
    area.ofereceram(id, ClipKind::Text, 2, *blake3::hash(&bytes).as_bytes());
    area.pedaco(id, 0, &bytes);
    assert!(area.fim(id).is_none());
}

#[test]
fn oferta_grande_demais_ou_de_outro_tipo_e_recusada_com_motivo() {
    let mut area = Area::default();
    let grande = u32::try_from(MAX_CLIPBOARD_TEXT_OFF_TCP + 1).unwrap();
    assert!(matches!(
        area.ofereceram(ClipId(1), ClipKind::Text, grande, [0; 32]),
        ClipboardMessage::Decline {
            reason: DeclineReason::TooLargeForCarrier,
            ..
        }
    ));
    assert!(matches!(
        area.ofereceram(ClipId(2), ClipKind::Image, 10, [0; 32]),
        ClipboardMessage::Decline {
            reason: DeclineReason::KindNotSupported,
            ..
        }
    ));
    assert!(area.montagem.is_none(), "reservou memória para uma recusa");
}

#[test]
fn pedido_de_oferta_substituida_nao_manda_nada() {
    let mut area = Area::default();
    let (antiga, ..) = oferta_de(&mut area, "primeiro");
    oferta_de(&mut area, "segundo");
    area.pedido(antiga);
    assert!(area.fila.is_empty());
}

#[test]
fn o_debug_nao_mostra_o_texto() {
    let mut area = Area::default();
    let (id, ..) = oferta_de(&mut area, "senha-do-banco");
    area.pedido(id);
    let visto = format!("{area:?}");
    assert!(!visto.contains("senha"), "{visto}");
}
