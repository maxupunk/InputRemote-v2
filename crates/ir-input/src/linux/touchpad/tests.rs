#![allow(clippy::unwrap_used, clippy::panic)]

use super::codigo::*;
use super::*;

/// Um touchpad de 1920 unidades de largura: uma unidade, um pixel.
fn touchpad() -> Touchpad {
    Touchpad::com_escala(1920, 1920.0)
}

/// Entrega um relato inteiro e devolve o que saiu.
fn relato(t: &mut Touchpad, eventos: &[Entrada], agora: Instant) -> Vec<CaptureEvent> {
    let mut saida = Vec::new();
    for &e in eventos.iter().chain(std::iter::once(&Entrada::Fim)) {
        saida.extend(t.evento(e, agora).unwrap_or_default());
    }
    saida
}

fn encosta(dedo: u16, x: i32, y: i32) -> Vec<Entrada> {
    vec![
        Entrada::Tecla(BTN_TOUCH, 1),
        Entrada::Tecla(dedo, 1),
        Entrada::Eixo(ABS_X, x),
        Entrada::Eixo(ABS_Y, y),
    ]
}

fn move_para(x: i32, y: i32) -> Vec<Entrada> {
    vec![Entrada::Eixo(ABS_X, x), Entrada::Eixo(ABS_Y, y)]
}

fn solta() -> Vec<Entrada> {
    vec![
        Entrada::Tecla(BTN_TOUCH, 0),
        Entrada::Tecla(BTN_TOOL_FINGER, 0),
    ]
}

#[test]
fn arrastar_o_dedo_vira_deslocamento_e_encostar_nao_salta() {
    let (mut t, t0) = (touchpad(), Instant::now());
    assert!(
        relato(&mut t, &encosta(BTN_TOOL_FINGER, 500, 400), t0).is_empty(),
        "encostar não é movimento: a posição absoluta não vira salto"
    );
    assert_eq!(
        relato(&mut t, &move_para(530, 390), t0),
        vec![CaptureEvent::PointerMotion { dx: 30, dy: -10 }]
    );
    relato(&mut t, &solta(), t0 + Duration::from_secs(1));
    // Outro toque em outro lugar da superfície recomeça dali, sem pular.
    assert!(relato(&mut t, &encosta(BTN_TOOL_FINGER, 100, 100), t0).is_empty());
}

#[test]
fn a_escala_vem_da_largura_do_eixo() {
    // Um touchpad de 3840 unidades e 1920 pixels de ponta a ponta: 200 unidades andam 100.
    let (mut t, t0) = (Touchpad::com_escala(3840, 1920.0), Instant::now());
    relato(&mut t, &encosta(BTN_TOOL_FINGER, 0, 0), t0);
    assert_eq!(
        relato(&mut t, &move_para(200, 0), t0),
        vec![CaptureEvent::PointerMotion { dx: 100, dy: 0 }]
    );
    // E o movimento lento não se perde: meio pixel e meio pixel dão um.
    relato(&mut t, &move_para(201, 0), t0);
    assert_eq!(
        relato(&mut t, &move_para(202, 0), t0),
        vec![CaptureEvent::PointerMotion { dx: 1, dy: 0 }]
    );
}

#[test]
fn um_toque_curto_e_parado_e_clique_esquerdo() {
    let (mut t, t0) = (touchpad(), Instant::now());
    relato(&mut t, &encosta(BTN_TOOL_FINGER, 500, 400), t0);
    let saida = relato(&mut t, &solta(), t0 + Duration::from_millis(80));
    assert_eq!(
        saida,
        vec![
            CaptureEvent::Button {
                button: Button::Left,
                pressed: true
            },
            CaptureEvent::Button {
                button: Button::Left,
                pressed: false
            },
        ]
    );
}

#[test]
fn toque_demorado_ou_arrastado_nao_e_clique() {
    let (mut t, t0) = (touchpad(), Instant::now());
    relato(&mut t, &encosta(BTN_TOOL_FINGER, 500, 400), t0);
    assert!(relato(&mut t, &solta(), t0 + Duration::from_millis(400)).is_empty());

    relato(&mut t, &encosta(BTN_TOOL_FINGER, 500, 400), t0);
    relato(&mut t, &move_para(600, 400), t0);
    assert!(relato(&mut t, &solta(), t0 + Duration::from_millis(80)).is_empty());
}

#[test]
fn dois_dedos_rolam_e_o_toque_de_dois_e_clique_direito() {
    let (mut t, t0) = (touchpad(), Instant::now());
    relato(&mut t, &encosta(BTN_TOOL_DOUBLETAP, 500, 400), t0);
    // O dedo desce 60 pixels: uma marcação, com o conteúdo acompanhando o dedo.
    assert_eq!(
        relato(&mut t, &move_para(500, 460), t0),
        vec![CaptureEvent::Wheel(WheelDelta {
            dx: 0,
            dy: WheelDelta::NOTCH
        })]
    );
    relato(&mut t, &solta(), t0 + Duration::from_secs(1));

    relato(&mut t, &encosta(BTN_TOOL_DOUBLETAP, 500, 400), t0);
    let saida = relato(&mut t, &solta(), t0 + Duration::from_millis(80));
    assert!(
        matches!(
            saida.first(),
            Some(CaptureEvent::Button {
                button: Button::Right,
                ..
            })
        ),
        "{saida:?}"
    );
}

#[test]
fn um_toque_de_um_dedo_depois_de_rolar_com_dois_e_clique_esquerdo() {
    let (mut t, t0) = (touchpad(), Instant::now());
    relato(&mut t, &encosta(BTN_TOOL_DOUBLETAP, 500, 400), t0);
    relato(&mut t, &solta(), t0 + Duration::from_secs(1));
    relato(&mut t, &encosta(BTN_TOOL_FINGER, 500, 400), t0);
    let saida = relato(&mut t, &solta(), t0 + Duration::from_millis(80));
    assert!(
        matches!(
            saida.first(),
            Some(CaptureEvent::Button {
                button: Button::Left,
                ..
            })
        ),
        "{saida:?}"
    );
}

#[test]
fn o_clique_fisico_segue_o_caminho_comum_e_o_toque_nao_clica_de_novo() {
    let (mut t, t0) = (touchpad(), Instant::now());
    relato(&mut t, &encosta(BTN_TOOL_FINGER, 500, 400), t0);
    assert_eq!(t.evento(Entrada::Tecla(BTN_LEFT, 1), t0), None);
    assert_eq!(t.evento(Entrada::Tecla(BTN_LEFT, 0), t0), None);
    assert!(relato(&mut t, &solta(), t0 + Duration::from_millis(80)).is_empty());
}

#[test]
fn trocar_de_dedo_no_meio_nao_salta() {
    let (mut t, t0) = (touchpad(), Instant::now());
    relato(&mut t, &encosta(BTN_TOOL_FINGER, 500, 400), t0);
    // Um segundo dedo encosta longe: a posição passa a ser dele.
    let mut dois = vec![
        Entrada::Tecla(BTN_TOOL_FINGER, 0),
        Entrada::Tecla(BTN_TOOL_DOUBLETAP, 1),
    ];
    dois.extend(move_para(1500, 900));
    assert!(relato(&mut t, &dois, t0).is_empty());
}

#[test]
fn a_passada_inteira_anda_menos_que_uma_tela() {
    // Devagar, uma passada não atravessa a tela inteira; a aceleração é que leva longe.
    let (mut t, t0) = (Touchpad::novo(3000), Instant::now());
    relato(&mut t, &encosta(BTN_TOOL_FINGER, 0, 0), t0);
    let Some(CaptureEvent::PointerMotion { dx, .. }) =
        relato(&mut t, &move_para(3000, 0), t0).first().copied()
    else {
        panic!("andou");
    };
    assert!(dx < 1920, "{dx}");
}
