#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::sync::{Arc, Mutex};

use ir_input::{CaptureEvent, InjectEvent, Injector};
use ir_proto::input::{Button, PointerPosition};
use ir_proto::screens::ScreenLayout;
use ir_session::Role;

use crate::actor::Daemon;
use crate::actor::bancada::{Bancada, CapturaDeMentira};

/// Um injetor que só anota o que recebeu.
#[derive(Clone, Default)]
struct Anotador(Arc<Mutex<Vec<InjectEvent>>>);

impl Injector for Anotador {
    fn inject(&mut self, event: InjectEvent) -> ir_input::Result<()> {
        self.0.lock().unwrap().push(event);
        Ok(())
    }

    fn release_all(&mut self) -> ir_input::Result<()> {
        Ok(())
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
    let mut bancada = Bancada::nova(Role::Server);
    let anotador = Anotador::default();
    bancada.daemon.capturer = Some(Box::new(CapturaDeMentira));
    bancada.daemon.injector = Some(Box::new(anotador.clone()));
    bancada
        .daemon
        .definir_telas(ScreenLayout::single(1920, 1080).unwrap());
    bancada.daemon.ajustar_conducao();
    (bancada, anotador)
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
fn uma_sessao_recriada_parte_de_onde_o_cursor_esta() {
    // Trocar de papel ou reconectar cria uma sessão nova; sem isto o cursor saltaria para onde
    // ela nasceu.
    let (mut bancada, anotador) = conduzindo();
    mover(&mut bancada.daemon, 100, 50);
    let antes = bancada.daemon.session.pointer_xy();
    bancada.daemon.session.sync_pointer(0, 0);
    anotador.tirar();
    mover(&mut bancada.daemon, 1, 0);
    assert_eq!(bancada.daemon.session.pointer_xy(), (antes.0 + 1, antes.1));
}

#[test]
fn controlado_ou_sem_injetor_nao_conduz() {
    let mut cliente = Bancada::nova(Role::Client);
    cliente.daemon.capturer = Some(Box::new(CapturaDeMentira));
    cliente.daemon.injector = Some(Box::new(Anotador::default()));
    cliente.daemon.ajustar_conducao();
    assert!(!cliente.daemon.cursor.ligada);

    let mut sem_injetor = Bancada::nova(Role::Server);
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
