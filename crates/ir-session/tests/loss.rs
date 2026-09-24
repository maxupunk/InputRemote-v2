//! A confiabilidade sobre datagrama, exercida através da sessão inteira.
//!
//! Os testes de `tests/reliability.rs` verificam a camada isolada. Estes verificam que ela
//! está **ligada**: que uma repetição não é injetada duas vezes, que digitação contínua não
//! enche a janela, e que perda persistente derruba o enlace em vez de prosseguir com lacuna.
//!
//! É a distinção entre "o mecanismo funciona" e "o produto usa o mecanismo". Uma camada
//! correta e desligada é indistinguível de uma camada ausente.

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

mod common;

use common::{Pair, Side, is};
use ir_proto::carrier::Carrier;
use ir_proto::input::{HidUsage, PointerDelta};
use ir_session::event::Notice;
use ir_session::{Input, Phase};

fn engaged() -> Pair {
    let mut pair = Pair::matched();
    pair.connect(Carrier::Udp);
    pair.feed(
        Side::Server,
        Input::LocalPointer(PointerDelta { dx: 5000, dy: 0 }),
    );
    assert_eq!(pair.client.phase(), Phase::Receiving);
    pair.clear_log();
    pair
}

#[test]
fn a_duplicated_frame_is_injected_only_once() {
    let mut pair = engaged();
    pair.set_duplicating(true);

    pair.feed(
        Side::Server,
        Input::LocalKey {
            usage: HidUsage(0x04),
            pressed: true,
        },
    );

    let injections = pair.count(Side::Client, is::injection);
    assert_eq!(
        injections, 1,
        "o meio entregou duas vezes; injetar duas seria digitar a tecla duas vezes"
    );
}

#[test]
fn a_duplicated_release_does_not_unbalance_the_state() {
    let mut pair = engaged();
    pair.set_duplicating(true);

    pair.feed(
        Side::Server,
        Input::LocalKey {
            usage: HidUsage(0x04),
            pressed: true,
        },
    );
    pair.feed(
        Side::Server,
        Input::LocalKey {
            usage: HidUsage(0x04),
            pressed: false,
        },
    );

    assert!(pair.client.input_state().is_released());
}

#[test]
fn continuous_typing_does_not_fill_the_window() {
    // O caso que derrubaria a sessão no meio de uma frase: o servidor manda tecla após tecla
    // e o cliente não tem nada a dizer, então não haveria quadro para carregar a confirmação.
    // A confirmação pura periódica é o que resolve.
    let mut pair = engaged();

    for round in 0..200u16 {
        let usage = HidUsage(0x04 + (round % 20));
        pair.feed(
            Side::Server,
            Input::LocalKey {
                usage,
                pressed: true,
            },
        );
        pair.feed(
            Side::Server,
            Input::LocalKey {
                usage,
                pressed: false,
            },
        );
        // A periferia entrega o `Tick` que a sessão pediu.
        pair.advance(25);
    }

    assert_eq!(
        pair.server.phase(),
        Phase::Sending,
        "200 teclas não podem derrubar a sessão: a janela tem de estar sendo liberada"
    );
    assert!(
        pair.client.input_state().is_released(),
        "e nada pode ter ficado pressionado"
    );
}

#[test]
fn an_occasional_loss_is_repaired_without_leaving_a_key_held() {
    let mut pair = engaged();
    pair.set_drop_every(7); // um em cada sete quadros se perde

    for round in 0..60u16 {
        let usage = HidUsage(0x04 + (round % 10));
        pair.feed(
            Side::Server,
            Input::LocalKey {
                usage,
                pressed: true,
            },
        );
        pair.advance(25);
        pair.feed(
            Side::Server,
            Input::LocalKey {
                usage,
                pressed: false,
            },
        );
        pair.advance(25);
    }

    assert!(
        pair.dropped() > 0,
        "o cenário precisa de fato perder quadros"
    );

    // Um segundo é o prazo prometido em docs/01-visao-e-escopo.md §6 para o retorno do
    // controle depois de o par sumir. Dentro dele, nada pode continuar pressionado — seja
    // porque a retransmissão reparou, porque o snapshot reconciliou, ou porque o enlace caiu
    // e os dois lados soltaram.
    for _ in 0..40 {
        pair.advance(25);
    }

    assert!(
        pair.client.input_state().is_released(),
        "com perda de 1 em 7, nada pode ficar pressionado depois de 1 s: sobrou {:?}",
        pair.client.input_state()
    );
    assert!(
        pair.server.input_state().is_released(),
        "e o servidor também não pode achar que algo continua pressionado"
    );
}

#[test]
fn total_loss_drops_the_link_instead_of_going_on_with_a_gap() {
    let mut pair = engaged();
    pair.feed(
        Side::Server,
        Input::LocalKey {
            usage: HidUsage(0x04),
            pressed: true,
        },
    );
    pair.set_delivery(false);
    pair.clear_log();

    // Tempo suficiente para todas as retransmissões vencerem e o enlace desistir.
    for _ in 0..60 {
        pair.advance(25);
    }

    assert_eq!(
        pair.server.phase(),
        Phase::Offline,
        "esgotadas as tentativas, prosseguir seguiria com lacuna no canal de teclado"
    );
    assert!(
        pair.any(Side::Server, is::release_all),
        "e ao desistir tem de soltar tudo"
    );
    assert!(pair.server.input_state().is_released());
}

#[test]
fn a_stall_shorter_than_the_promised_second_does_not_drop_the_session() {
    // O Wi-Fi com economia de energia segura quadros por mais de 100 ms de vez em quando. Isso
    // tem de virar latência, e não queda: o usuário não pode ver "Desconectado" por causa de um
    // pico, e o prazo prometido para declarar o par perdido é de 1 s (log 23).
    let mut pair = engaged();
    pair.feed(
        Side::Server,
        Input::LocalKey {
            usage: HidUsage(0x04),
            pressed: true,
        },
    );
    pair.clear_log();

    pair.set_delivery(false);
    for _ in 0..40 {
        pair.advance(10); // 400 ms sem nada passar, em nenhum sentido
    }
    pair.set_delivery(true);
    for _ in 0..40 {
        pair.advance(50); // e 2 s de normalidade depois
    }

    let dropped = |side: Side| {
        pair.notices(side)
            .iter()
            .any(|notice| matches!(notice, Notice::Disconnected { .. }))
    };
    assert!(
        !dropped(Side::Server),
        "o servidor derrubou a sessão por um pico de 400 ms"
    );
    assert!(
        !dropped(Side::Client),
        "o cliente derrubou a sessão por um pico de 400 ms"
    );
    assert!(pair.server.phase().is_established());
    assert!(pair.client.phase().is_established());

    // E a tecla que atravessou o pico continua coerente dos dois lados: soltá-la solta.
    pair.feed(
        Side::Server,
        Input::LocalKey {
            usage: HidUsage(0x04),
            pressed: false,
        },
    );
    for _ in 0..10 {
        pair.advance(25);
    }
    assert!(pair.client.input_state().is_released());
    assert!(pair.server.input_state().is_released());
}

#[test]
fn a_stream_carrier_also_uses_the_window_and_confirmations_drain_it() {
    // Desde a versão 2 a sessão trata o RFCOMM como datagrama (ADR-0012): ele também confirma, e
    // é isso que deixa o Bluetooth entrar e sair da rota sem refazer a sessão. Num meio que não
    // perde, a janela só esvazia — digitar sem parar não pode derrubar nada.
    let mut pair = Pair::matched();
    pair.connect(Carrier::Rfcomm);
    pair.feed(
        Side::Server,
        Input::LocalPointer(PointerDelta { dx: 5000, dy: 0 }),
    );
    pair.clear_log();

    for round in 0..100u16 {
        let usage = HidUsage(0x04 + (round % 20));
        pair.feed(
            Side::Server,
            Input::LocalKey {
                usage,
                pressed: true,
            },
        );
        pair.feed(
            Side::Server,
            Input::LocalKey {
                usage,
                pressed: false,
            },
        );
        // Uma tecla a cada 10 ms, que é digitar rápido; a confirmação pura sai a cada 20 ms.
        pair.advance(10);
    }

    assert_eq!(
        pair.server.phase(),
        Phase::Sending,
        "a janela esvazia a cada confirmação"
    );
    assert!(pair.client.input_state().is_released());
}
