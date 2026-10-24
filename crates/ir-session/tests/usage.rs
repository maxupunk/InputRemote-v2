//! O painel no Android: o recado entre os computadores, e a latência por rota.
//!
//! O que se prova: o recado só anda com sessão de pé e dentro do teto — e o maior cabe no quadro do
//! rádio —; e na rota dupla cada portador tem a sua volta medida, embora só um `Pong` seja aplicado.
#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

mod common;

use common::{Pair, Side};
use ir_proto::carrier::Carrier;
use ir_session::event::Notice;
use ir_session::{Input, Route};

fn recados(pair: &Pair, side: Side) -> Vec<Vec<u8>> {
    pair.notices(side)
        .iter()
        .filter_map(|notice| match notice {
            Notice::PeerPanel(recado) => Some(recado.clone()),
            _ => None,
        })
        .collect()
}

fn rotas_medidas(pair: &Pair, side: Side) -> Vec<Carrier> {
    let mut carriers: Vec<Carrier> = pair
        .notices(side)
        .iter()
        .filter_map(|notice| match notice {
            Notice::RouteLatency { carrier, .. } => Some(*carrier),
            _ => None,
        })
        .collect();
    carriers.sort_unstable();
    carriers.dedup();
    carriers
}

/// Um segundo em batidas de 250 ms — um avanço só de 1 s passaria do prazo de queda.
fn batidas(pair: &mut Pair) {
    for _ in 0..4 {
        pair.advance(250);
    }
}

#[test]
fn o_recado_do_painel_chega_ao_par_e_so_ao_par() {
    let mut pair = Pair::matched();
    pair.connect(Carrier::Udp);
    pair.clear_log();
    assert!(pair.server.peer_has_panel());

    pair.feed(Side::Server, Input::Panel(vec![7, 8, 9]));
    pair.advance(50);
    assert_eq!(recados(&pair, Side::Client), vec![vec![7, 8, 9]]);
    assert!(
        recados(&pair, Side::Server).is_empty(),
        "o recado não volta"
    );
}

#[test]
fn um_recado_maior_que_o_teto_nao_sai() {
    let mut pair = Pair::matched();
    pair.connect(Carrier::Rfcomm);
    pair.clear_log();
    let grande = vec![0u8; ir_proto::limits::MAX_PANEL_MESSAGE + 1];
    pair.feed(Side::Server, Input::Panel(grande));
    pair.advance(50);
    assert!(recados(&pair, Side::Client).is_empty());
}

#[test]
fn o_maior_recado_cabe_no_quadro_do_radio() {
    let mut pair = Pair::matched();
    pair.connect(Carrier::Rfcomm);
    pair.clear_log();
    let maior = vec![0xAB; ir_proto::limits::MAX_PANEL_MESSAGE];
    pair.feed(Side::Server, Input::Panel(maior.clone()));
    pair.advance(50);
    assert_eq!(recados(&pair, Side::Client), vec![maior]);
}

#[test]
fn sem_sessao_nao_ha_a_quem_mandar() {
    let mut pair = Pair::matched();
    assert!(!pair.server.peer_has_panel());
    pair.feed(Side::Server, Input::Panel(vec![1]));
    pair.advance(50);
    assert!(recados(&pair, Side::Client).is_empty());
}

#[test]
fn na_rota_dupla_cada_portador_tem_a_sua_volta() {
    let mut pair = Pair::matched();
    pair.connect(Carrier::Udp);
    pair.connect(Carrier::Rfcomm);
    assert_eq!(pair.server.route(), Some(Route::Dual));
    pair.clear_log();

    // Algumas batidas, abaixo do prazo de queda: o `Ping` sai pelos dois, e o `Pong` volta pelos
    // dois.
    batidas(&mut pair);

    assert_eq!(
        rotas_medidas(&pair, Side::Server),
        vec![Carrier::Rfcomm, Carrier::Udp]
    );
}

#[test]
fn com_um_portador_so_ele_e_medido() {
    let mut pair = Pair::matched();
    pair.connect(Carrier::Udp);
    pair.clear_log();
    batidas(&mut pair);
    assert_eq!(rotas_medidas(&pair, Side::Server), vec![Carrier::Udp]);
}
