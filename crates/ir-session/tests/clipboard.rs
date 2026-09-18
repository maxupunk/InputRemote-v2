//! O canal 4 exercido através das duas sessões: o texto copiado de um lado chega inteiro ao outro.
//!
//! O que importa aqui não é só "chega". É que um texto grande, sobre um meio que perde quadros,
//! chega **sem derrubar o enlace** — a janela cheia derrubaria — e sem o teclado esperar atrás dele.

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

mod common;

use common::{Pair, Side};
use ir_proto::carrier::Carrier;
use ir_proto::input::HidUsage;
use ir_proto::limits::MAX_CLIPBOARD_TEXT_OFF_TCP;
use ir_session::event::Notice;
use ir_session::{ClipText, Command, Injection, Input};

fn texto(conteudo: &str) -> ClipText {
    ClipText::new(conteudo.to_owned()).unwrap()
}

/// Um texto grande e que muda ao longo do corpo, para um pedaço trocado de lugar aparecer.
fn texto_longo(bytes: usize) -> String {
    (0..bytes)
        .map(|i| char::from(b'a' + u8::try_from(i % 26).unwrap()))
        .collect()
}

/// Os textos que chegaram a este lado.
fn recebidos(pair: &Pair, side: Side) -> Vec<String> {
    pair.commands(side)
        .into_iter()
        .filter_map(|command| match command {
            Command::ClipboardText(texto) => Some(texto.into_string()),
            _ => None,
        })
        .collect()
}

fn caiu(pair: &Pair) -> bool {
    [Side::Server, Side::Client].into_iter().any(|side| {
        pair.notices(side)
            .iter()
            .any(|notice| matches!(notice, Notice::Disconnected { .. }))
    })
}

/// Avança batidas de 5 ms, como o serviço, até o texto chegar ou o prazo acabar.
fn ate_chegar(pair: &mut Pair, side: Side, batidas: u32) {
    for _ in 0..batidas {
        if !recebidos(pair, side).is_empty() {
            return;
        }
        pair.advance(5);
    }
}

fn conectado(carrier: Carrier) -> Pair {
    let mut pair = Pair::matched();
    pair.connect(carrier);
    pair.clear_log();
    pair
}

#[test]
fn texto_curto_atravessa_nos_dois_sentidos() {
    let mut pair = conectado(Carrier::Udp);
    pair.feed(Side::Server, Input::ClipboardText(texto("olá, cliente\n")));
    ate_chegar(&mut pair, Side::Client, 10);
    assert_eq!(recebidos(&pair, Side::Client), vec!["olá, cliente\n"]);

    pair.clear_log();
    pair.feed(Side::Client, Input::ClipboardText(texto("e de volta")));
    ate_chegar(&mut pair, Side::Server, 10);
    assert_eq!(recebidos(&pair, Side::Server), vec!["e de volta"]);
}

#[test]
fn o_maior_texto_do_canal_atravessa_uma_rede_que_perde_sem_derrubar_o_enlace() {
    let mut pair = conectado(Carrier::Udp);
    pair.set_drop_every(5);
    let grande = texto_longo(MAX_CLIPBOARD_TEXT_OFF_TCP);
    pair.feed(
        Side::Server,
        Input::ClipboardText(ClipText::new(grande.clone()).unwrap()),
    );
    // Um quadro em cada cinco perdido. 256 KiB em pedaços de 448 B, dois por batida: ~600
    // pedaços, ~300 batidas sem perda.
    ate_chegar(&mut pair, Side::Client, 3_000);
    assert!(pair.dropped() > 0, "o teste precisa de perda para valer");
    let quedas: Vec<_> = [Side::Server, Side::Client]
        .into_iter()
        .flat_map(|side| pair.notices(side))
        .filter(|notice| matches!(notice, Notice::Disconnected { .. }))
        .collect();
    assert!(
        quedas.is_empty(),
        "o enlace caiu no meio do texto: {quedas:?}"
    );
    assert_eq!(recebidos(&pair, Side::Client), vec![grande]);
}

#[test]
fn sobre_bluetooth_tambem_chega() {
    let mut pair = conectado(Carrier::Rfcomm);
    let medio = texto_longo(40_000);
    pair.feed(
        Side::Client,
        Input::ClipboardText(ClipText::new(medio.clone()).unwrap()),
    );
    ate_chegar(&mut pair, Side::Server, 1_000);
    assert_eq!(recebidos(&pair, Side::Server), vec![medio]);
}

#[test]
fn o_teclado_nao_espera_atras_do_texto() {
    let mut pair = conectado(Carrier::Udp);
    pair.feed(
        Side::Server,
        Input::LocalPointer(ir_proto::input::PointerDelta { dx: 5000, dy: 0 }),
    );
    pair.feed(
        Side::Server,
        Input::ClipboardText(ClipText::new(texto_longo(200_000)).unwrap()),
    );
    pair.advance(5);
    pair.clear_log();
    // Com o texto ainda na fila, uma tecla sai e é injetada no mesmo passo.
    pair.feed(
        Side::Server,
        Input::LocalKey {
            usage: HidUsage(0x04),
            pressed: true,
        },
    );
    assert!(
        pair.commands(Side::Client)
            .iter()
            .any(|c| matches!(c, Command::Inject(Injection::Key { pressed: true, .. }))),
        "a tecla ficou atrás do clipboard"
    );
    assert!(recebidos(&pair, Side::Client).is_empty(), "ainda chegando");
}

#[test]
fn copiar_de_novo_no_meio_substitui_o_anterior() {
    let mut pair = conectado(Carrier::Udp);
    pair.feed(
        Side::Server,
        Input::ClipboardText(ClipText::new(texto_longo(100_000)).unwrap()),
    );
    pair.advance(5);
    pair.feed(Side::Server, Input::ClipboardText(texto("o mais novo")));
    ate_chegar(&mut pair, Side::Client, 1_000);
    for _ in 0..200 {
        pair.advance(5);
    }
    assert_eq!(recebidos(&pair, Side::Client), vec!["o mais novo"]);
}

#[test]
fn sem_sessao_nada_e_oferecido() {
    let mut pair = Pair::matched();
    pair.feed(Side::Server, Input::ClipboardText(texto("fora de hora")));
    assert!(
        !pair
            .commands(Side::Server)
            .iter()
            .any(|c| matches!(c, Command::Send { .. })),
        "ofereceu sem sessão"
    );
}

#[test]
fn a_queda_esquece_o_que_estava_indo() {
    let mut pair = conectado(Carrier::Udp);
    pair.feed(
        Side::Server,
        Input::ClipboardText(ClipText::new(texto_longo(100_000)).unwrap()),
    );
    pair.advance(5);
    pair.set_delivery(false);
    pair.advance(2_000); // o prazo de queda
    assert!(caiu(&pair));
    pair.set_delivery(true);
    pair.clear_log();
    pair.connect(Carrier::Udp);
    for _ in 0..600 {
        pair.advance(5);
    }
    assert!(
        recebidos(&pair, Side::Client).is_empty(),
        "texto de uma sessão encerrada apareceu na seguinte"
    );
}
