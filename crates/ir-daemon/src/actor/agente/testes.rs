//! O ciclo de vida do agente, visto do ator: pronto, comandos, renovação e queda.

use ir_ipc::{ComandoDoAgente, FatoDoAgente};
use ir_proto::input::HidUsage;
use ir_session::{Command, Injection};

use super::super::bancada::Bancada;

fn pronto(bancada: &mut Bancada) {
    bancada.daemon.on_fato(FatoDoAgente::Pronto {
        desktops: vec!["Default".to_owned()],
        tela_de_bloqueio: false,
    });
    // O agente que chega fica sabendo, antes de tudo, se pode digitar na tela de bloqueio.
    assert_eq!(
        recebidos(bancada),
        vec![ComandoDoAgente::PermitirDesktopProtegido(false)]
    );
}

fn recebidos(bancada: &mut Bancada) -> Vec<ComandoDoAgente> {
    let mut todos = Vec::new();
    while let Ok(comando) = bancada.agente.try_recv() {
        todos.push(comando);
    }
    todos
}

#[test]
fn sem_agente_nada_vai_para_o_canal_dele() {
    let mut bancada = Bancada::nova();
    assert!(bancada.daemon.comandos_do_agente().is_none());
    bancada.daemon.out.push(Command::ReleaseAll);
    bancada.daemon.apply_commands();
    assert!(recebidos(&mut bancada).is_empty());
}

#[test]
fn com_agente_pronto_a_injecao_e_a_soltura_vao_para_ele() {
    let mut bancada = Bancada::nova();
    pronto(&mut bancada);
    let tecla = HidUsage(0x04);
    bancada.daemon.out.push(Command::Inject(Injection::Key {
        usage: tecla,
        pressed: true,
    }));
    bancada.daemon.out.push(Command::ReleaseAll);
    bancada.daemon.apply_commands();

    assert_eq!(
        recebidos(&mut bancada),
        vec![
            ComandoDoAgente::Injetar(Injection::Key {
                usage: tecla,
                pressed: true
            }),
            ComandoDoAgente::SoltarTudo,
        ]
    );
}

#[test]
fn a_supressao_e_renovada_enquanto_vale() {
    let mut bancada = Bancada::nova();
    pronto(&mut bancada);
    bancada.daemon.out.push(Command::SuppressLocalInput(true));
    bancada.daemon.apply_commands();
    let _ = recebidos(&mut bancada);

    bancada.daemon.renovar_supressao();
    assert_eq!(
        recebidos(&mut bancada),
        vec![ComandoDoAgente::SuprimirEntradaLocal(true)],
        "o vigia do agente devolve o teclado se a renovação parar"
    );

    bancada.daemon.out.push(Command::SuppressLocalInput(false));
    bancada.daemon.apply_commands();
    let _ = recebidos(&mut bancada);
    bancada.daemon.renovar_supressao();
    assert!(
        recebidos(&mut bancada).is_empty(),
        "desligada, não se renova"
    );
}

#[test]
fn o_agente_que_sai_deixa_de_receber_e_a_janela_fica_sabendo() {
    let mut bancada = Bancada::nova();
    let mut avisos = bancada.daemon.avisos.subscribe();
    pronto(&mut bancada);
    assert!(bancada.daemon.comandos_do_agente().is_some());

    bancada.daemon.on_fato(FatoDoAgente::Encerrou);

    assert!(bancada.daemon.comandos_do_agente().is_none());
    let mut mudou = 0;
    while let Ok(aviso) = avisos.try_recv() {
        if matches!(aviso, ir_ipc::Aviso::EstadoMudou(_)) {
            mudou += 1;
        }
    }
    assert!(mudou >= 2, "pronto e encerrado aparecem na janela");
}

#[test]
fn a_recusa_na_area_de_trabalho_aparece_na_tela_e_some_quando_para() {
    // O log 49: o sistema recusava toda injeção, o par dizia "controlando", e esta tela não dizia
    // nada.
    let mut bancada = Bancada::nova();
    pronto(&mut bancada);
    assert!(bancada.daemon.estado().agente_pronto);

    bancada.daemon.on_fato(FatoDoAgente::InjecaoRecusada {
        desktop: "Default".to_owned(),
        protegido: false,
    });
    assert!(!bancada.daemon.estado().agente_pronto, "a tela avisa");
    assert!(
        !bancada.daemon.recusa_protegido,
        "a área de trabalho não é o desktop protegido"
    );

    // Sem recusa por mais de alguns segundos, a injeção voltou a passar.
    bancada.daemon.injecao_recusada =
        std::time::Instant::now().checked_sub(std::time::Duration::from_secs(6));
    bancada.daemon.esquecer_recusa_antiga();
    assert!(bancada.daemon.estado().agente_pronto);
}
