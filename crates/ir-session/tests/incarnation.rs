//! Encarnações de sessão: quadros de uma sessão que já acabou não podem mexer na seguinte.
//!
//! O defeito que estes testes reproduzem apareceu no teste físico Windows → Linux por Wi-Fi: logo
//! depois de parear, os dois lados passaram a registrar `sessão encerrada reason=Timeout` a cada
//! ~200 ms, sem parar. Um pico de latência derrubou a sessão de um lado; o outro continuou mandando
//! quadros com a numeração antiga; o lado que acabara de zerar se ancorou num desses quadros velhos;
//! e o `Hello` novo do par, com número 1, passou a parecer mais velho que a âncora e foi descartado
//! calado. Sem resposta, o par retransmitia, desistia, e recomeçava.
//!
//! Nada no quadro dizia a qual sessão ele pertencia. Estes cenários fixam o comportamento certo: o
//! que é de uma encarnação anterior é ignorado, e uma encarnação nova do par é reconhecida.
#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

mod common;

use common::{Pair, Side};
use ir_proto::carrier::Carrier;
use ir_proto::frame::Frame;
use ir_proto::message::{Control, Message};
use ir_session::event::{LinkDown, Notice};
use ir_session::{Command, Input, Phase};

/// Os dois lados de pé, com tráfego já trocado: a numeração do canal de controle passou de 1.
fn connected_with_traffic() -> Pair {
    let mut pair = Pair::matched();
    pair.connect(Carrier::Udp);
    for _ in 0..10 {
        pair.advance(100);
    }
    assert_eq!(pair.server.phase(), Phase::Ready);
    assert_eq!(pair.client.phase(), Phase::Ready);
    pair.clear_log();
    pair
}

/// Os quadros que um lado pediu para enviar desde a última limpeza, entregues ou não.
fn sent_frames(pair: &Pair, side: Side) -> Vec<(Carrier, Frame)> {
    pair.commands(side)
        .into_iter()
        .filter_map(|command| match command {
            Command::Send { carrier, frame } => Some((carrier, frame)),
            _ => None,
        })
        .collect()
}

/// Com a entrega desligada, deixa o heartbeat sair e guarda o `Ping` que o lado mandou.
///
/// É o quadro "velho": pertence à sessão corrente e vai chegar depois que ela acabar.
fn capture_ping(pair: &mut Pair, side: Side) -> (Carrier, Frame) {
    pair.set_delivery(false);
    pair.clear_log();
    pair.advance(210);
    let (carrier, frame) = sent_frames(pair, side)
        .into_iter()
        .rev()
        .find(|(_, frame)| matches!(frame.message, Message::Control(Control::Ping { .. })))
        .expect("o heartbeat deveria ter mandado um Ping");
    assert!(
        frame.seq.get() > 1,
        "o quadro velho precisa ter numeração acima do começo, senão o cenário não reproduz"
    );
    (carrier, frame)
}

fn fall(pair: &mut Pair, side: Side) {
    pair.feed(
        side,
        Input::CarrierDown {
            carrier: Carrier::Udp,
            reason: LinkDown::TransportFailed,
        },
    );
    assert_eq!(side_phase(pair, side), Phase::Offline);
}

fn side_phase(pair: &Pair, side: Side) -> Phase {
    match side {
        Side::Server => pair.server.phase(),
        Side::Client => pair.client.phase(),
    }
}

fn disconnections(pair: &Pair, side: Side) -> usize {
    pair.notices(side)
        .iter()
        .filter(|notice| matches!(notice, Notice::Disconnected { .. }))
        .count()
}

/// Deixa o tempo passar com entrega normal e confere que ninguém caiu nesse meio-tempo.
fn assert_stays_up(pair: &mut Pair, why: &str) {
    pair.clear_log();
    for _ in 0..30 {
        pair.advance(100);
    }
    assert_eq!(pair.server.phase(), Phase::Ready, "{why}: o servidor caiu");
    assert_eq!(pair.client.phase(), Phase::Ready, "{why}: o cliente caiu");
    assert_eq!(
        disconnections(pair, Side::Server),
        0,
        "{why}: o servidor encerrou a sessão"
    );
    assert_eq!(
        disconnections(pair, Side::Client),
        0,
        "{why}: o cliente encerrou a sessão"
    );
}

#[test]
fn a_stale_frame_does_not_anchor_the_next_session() {
    let mut pair = connected_with_traffic();
    let (carrier, stale) = capture_ping(&mut pair, Side::Client);

    // O pico de latência: os dois lados desistem sem conseguir se falar.
    fall(&mut pair, Side::Server);
    fall(&mut pair, Side::Client);

    // O `Ping` da sessão que acabou chega atrasado a quem já zerou tudo.
    pair.feed(
        Side::Server,
        Input::Received {
            carrier,
            frame: stale,
        },
    );

    pair.set_delivery(true);
    pair.clear_log();
    // O cliente tenta de novo, como o serviço faz.
    pair.feed(Side::Client, Input::CarrierUp(Carrier::Udp));

    assert_eq!(
        pair.server.phase(),
        Phase::Ready,
        "o Hello da sessão nova precisa ser aceito mesmo depois de um quadro da anterior"
    );
    assert_eq!(pair.client.phase(), Phase::Ready);
    assert_stays_up(&mut pair, "depois de um quadro velho");
}

#[test]
fn a_peer_that_restarted_without_saying_goodbye_is_recognized() {
    let mut pair = connected_with_traffic();

    // O cliente cai sozinho, e o adeus dele se perde no caminho.
    pair.set_delivery(false);
    fall(&mut pair, Side::Client);
    assert_eq!(
        pair.server.phase(),
        Phase::Ready,
        "o servidor não soube de nada"
    );

    pair.set_delivery(true);
    pair.clear_log();
    pair.feed(Side::Client, Input::CarrierUp(Carrier::Udp));

    assert_eq!(
        pair.client.phase(),
        Phase::Ready,
        "o servidor precisa reconhecer uma sessão nova do par, mesmo sem ter visto a antiga acabar"
    );
    assert_eq!(pair.server.phase(), Phase::Ready);
    assert_stays_up(&mut pair, "depois de o par reiniciar sozinho");
}

#[test]
fn a_farewell_from_the_previous_session_does_not_end_the_current_one() {
    let mut pair = connected_with_traffic();

    pair.set_delivery(false);
    pair.clear_log();
    fall(&mut pair, Side::Client);
    let (carrier, stale_bye) = sent_frames(&pair, Side::Client)
        .into_iter()
        .find(|(_, frame)| matches!(frame.message, Message::Control(Control::Bye { .. })))
        .expect("quem cai de uma sessão de pé avisa o par");
    fall(&mut pair, Side::Server);

    pair.set_delivery(true);
    pair.connect(Carrier::Udp);
    assert_eq!(pair.server.phase(), Phase::Ready);
    assert_eq!(pair.client.phase(), Phase::Ready);

    pair.clear_log();
    pair.feed(
        Side::Server,
        Input::Received {
            carrier,
            frame: stale_bye,
        },
    );

    assert_eq!(
        pair.server.phase(),
        Phase::Ready,
        "um adeus da sessão anterior, chegando atrasado, não pode encerrar a atual"
    );
    assert_stays_up(&mut pair, "depois de um adeus velho");
}

#[test]
fn a_retransmitted_hello_does_not_restart_a_live_session() {
    // Proteção: o que já funcionava continua funcionando. Um Hello repetido da **mesma** sessão —
    // o HelloAck se perdeu e o par mandou de novo — não pode derrubar nem reanunciar nada.
    let mut pair = Pair::matched();
    pair.feed(Side::Client, Input::CarrierUp(Carrier::Udp));
    let (carrier, hello) = sent_frames(&pair, Side::Client)
        .into_iter()
        .find(|(_, frame)| matches!(frame.message, Message::Control(Control::Hello(_))))
        .expect("quem abre a sessão manda Hello");
    pair.feed(Side::Server, Input::CarrierUp(Carrier::Udp));
    for _ in 0..10 {
        pair.advance(100);
    }
    assert_eq!(pair.server.phase(), Phase::Ready);

    pair.clear_log();
    pair.feed(
        Side::Server,
        Input::Received {
            carrier,
            frame: hello,
        },
    );

    let reannounced = pair.notices(Side::Server).iter().any(|notice| {
        matches!(
            notice,
            Notice::Connected { .. } | Notice::Disconnected { .. }
        )
    });
    assert!(!reannounced, "um Hello repetido não é sessão nova");
    assert_stays_up(&mut pair, "depois de um Hello repetido");
}

#[test]
fn a_hello_from_an_older_session_does_not_restart_the_current_one() {
    // Proteção: a rede duplica e atrasa. Um Hello de uma encarnação **anterior** do par, chegando
    // depois que a atual já está de pé, não pode ser tomado por uma reinicialização.
    let mut pair = Pair::matched();
    pair.feed(Side::Client, Input::CarrierUp(Carrier::Udp));
    let (carrier, old_hello) = sent_frames(&pair, Side::Client)
        .into_iter()
        .find(|(_, frame)| matches!(frame.message, Message::Control(Control::Hello(_))))
        .expect("quem abre a sessão manda Hello");
    pair.feed(Side::Server, Input::CarrierUp(Carrier::Udp));
    assert_eq!(pair.server.phase(), Phase::Ready);

    // As duas pontas caem e voltam: agora há uma sessão nova.
    fall(&mut pair, Side::Server);
    fall(&mut pair, Side::Client);
    pair.connect(Carrier::Udp);
    for _ in 0..10 {
        pair.advance(100);
    }
    assert_eq!(pair.server.phase(), Phase::Ready);

    pair.clear_log();
    pair.feed(
        Side::Server,
        Input::Received {
            carrier,
            frame: old_hello,
        },
    );

    assert_stays_up(&mut pair, "depois de um Hello de uma sessão anterior");
}
