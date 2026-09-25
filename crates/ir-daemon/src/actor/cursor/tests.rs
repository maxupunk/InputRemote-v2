#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::sync::{Arc, Mutex};

use ir_input::{CaptureEvent, InjectEvent, Injector};
use ir_proto::input::{Button, PointerPosition};
use ir_proto::screens::ScreenLayout;

use crate::actor::Daemon;
use crate::actor::bancada::{Bancada, CapturaDeMentira};

/// Um injetor que só anota o que recebeu, e o último arranjo de telas.
#[derive(Clone, Default)]
struct Anotador(
    Arc<Mutex<Vec<InjectEvent>>>,
    Arc<Mutex<Option<ScreenLayout>>>,
);

impl Injector for Anotador {
    fn inject(&mut self, event: InjectEvent) -> ir_input::Result<()> {
        self.0.lock().unwrap().push(event);
        Ok(())
    }

    fn release_all(&mut self) -> ir_input::Result<()> {
        Ok(())
    }

    fn usar_telas(&mut self, telas: &ScreenLayout) {
        *self.1.lock().unwrap() = Some(telas.clone());
    }
}

impl Anotador {
    fn tirar(&self) -> Vec<InjectEvent> {
        std::mem::take(&mut *self.0.lock().unwrap())
    }

    /// A última posição absoluta posta no cursor.
    fn cursor(&self) -> Option<PointerPosition> {
        self.tirar().into_iter().rev().find_map(|e| match e {
            InjectEvent::Pointer(p) => Some(p),
            _ => None,
        })
    }
}

/// Um serviço com o teclado, que captura e injeta — o Linux com a condução ligada.
fn conduzindo() -> (Bancada, Anotador) {
    let mut bancada = Bancada::nova();
    let anotador = Anotador::default();
    bancada.daemon.capturer = Some(Box::new(CapturaDeMentira));
    bancada.daemon.injector = Some(Box::new(anotador.clone()));
    bancada
        .daemon
        .definir_telas(ScreenLayout::single(1920, 1080).unwrap());
    bancada.daemon.ajustar_conducao();
    (bancada, anotador)
}

#[test]
fn o_injetor_recebe_o_arranjo_local_para_por_o_ponteiro_no_monitor_certo() {
    // Sem o arranjo, o `uinput` entregava a fração de um monitor como fração do desktop inteiro.
    let (_bancada, anotador) = conduzindo();
    assert_eq!(
        *anotador.1.lock().unwrap(),
        Some(ScreenLayout::single(1920, 1080).unwrap())
    );
}

fn mover(daemon: &mut Daemon, dx: i32, dy: i32) {
    daemon.on_capture(CaptureEvent::PointerMotion { dx, dy });
}

#[test]
fn com_o_teclado_aqui_o_cursor_nasce_no_meio_e_segue_o_modelo() {
    let (mut bancada, anotador) = conduzindo();
    assert!(bancada.daemon.cursor.ligada);
    let meio = anotador.cursor().expect("o cursor foi posto no meio");
    assert!((32_000..33_600).contains(&meio.x), "{meio:?}");

    // Sem par nenhum: o cursor anda assim mesmo — é o serviço que o move.
    mover(&mut bancada.daemon, 480, 0);
    let depois = anotador.cursor().expect("o cursor andou");
    assert!(depois.x > meio.x + 15_000, "{meio:?} → {depois:?}");
    assert_eq!(bancada.daemon.session.pointer_xy().0, 1440);
}

#[test]
fn o_cursor_para_na_borda_sem_par_e_nao_sai_da_tela() {
    let (mut bancada, anotador) = conduzindo();
    mover(&mut bancada.daemon, 50_000, 0);
    let na_borda = anotador.cursor().expect("andou");
    assert_eq!(na_borda.x, u16::MAX, "encostou na borda direita");
    mover(&mut bancada.daemon, 50_000, 0);
    assert_eq!(anotador.cursor(), None, "parado na borda, nada a mover");
}

#[test]
fn botao_e_roda_tomados_chegam_ao_sistema_daqui() {
    let (mut bancada, anotador) = conduzindo();
    anotador.tirar();
    bancada.daemon.on_capture(CaptureEvent::Button {
        button: Button::Left,
        pressed: true,
    });
    assert_eq!(
        anotador.tirar(),
        vec![InjectEvent::Button {
            button: Button::Left,
            pressed: true
        }]
    );
}

#[test]
fn onde_o_outro_deixou_o_cursor_e_de_onde_a_mao_daqui_continua() {
    // O outro computador move o cursor daqui enquanto o usa; ao retomar, a mão daqui segue dali,
    // sem o cursor pular de volta para onde ela o tinha deixado.
    let (mut bancada, anotador) = conduzindo();
    mover(&mut bancada.daemon, 100, 50);
    bancada.daemon.session.sync_pointer(300, 200);
    anotador.tirar();
    mover(&mut bancada.daemon, 1, 0);
    assert_eq!(bancada.daemon.session.pointer_xy(), (301, 200));
}

#[test]
fn quem_nunca_controla_ou_nao_tem_injetor_nao_conduz() {
    let mut so_o_outro = Bancada::nova();
    so_o_outro.daemon.session = crate::actor::nova_sessao(
        ir_session::Policy::OnlyControlled,
        ir_proto::screens::Edge::Right,
        so_o_outro.daemon.identidade_local.clone(),
        None,
    );
    so_o_outro.daemon.capturer = Some(Box::new(CapturaDeMentira));
    so_o_outro.daemon.injector = Some(Box::new(Anotador::default()));
    so_o_outro.daemon.ajustar_conducao();
    assert!(!so_o_outro.daemon.cursor.ligada);

    let mut sem_injetor = Bancada::nova();
    sem_injetor.daemon.capturer = Some(Box::new(CapturaDeMentira));
    sem_injetor.daemon.ajustar_conducao();
    assert!(!sem_injetor.daemon.cursor.ligada);
}

#[test]
fn a_volta_do_par_poe_o_cursor_real_na_borda() {
    // Antes a posição da volta era ignorada no Linux, e o cursor real e o modelo nunca mais se
    // reencontravam (log 50).
    let (mut bancada, anotador) = conduzindo();
    anotador.tirar();
    let borda = PointerPosition {
        monitor: ir_proto::ids::MonitorId(0),
        x: u16::MAX,
        y: 30_000,
    };
    bancada.daemon.session.sync_pointer(1919, 494);
    bancada
        .daemon
        .out
        .push(ir_session::Command::WarpPointer(borda));
    bancada.daemon.apply_commands();
    let posto = anotador.cursor().expect("o cursor foi levado");
    assert_eq!(posto.x, u16::MAX);
}
