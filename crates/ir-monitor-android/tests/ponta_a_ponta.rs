//! O núcleo do aplicativo contra o monitor do serviço, os dois de verdade, no *loopback*: parear
//! conferindo o código, receber o quadro, voltar sem código pela chave gravada e, dito o interesse,
//! receber os detalhes junto do quadro.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::path::PathBuf;
use std::time::{Duration, Instant};

use ir_crypto::Identity;
use ir_monitor::{Copia, Detalhes, FaseDaCopia, Interesse, Maquina, Quadro, Situacao};
use ir_monitor_android::Ligacao;
use ir_monitor_android::cliente::{Comando, Destino, Evento};
use ir_monitor_android::cofre::Cofre;
use ir_monitor_rede::{Aviso, Comando as NoMonitor, Monitor};
use tokio::sync::mpsc;

const PRAZO: Duration = Duration::from_secs(10);

fn porta_livre() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

/// A pasta do cofre do teste, apagada no fim.
struct Pasta(PathBuf);

impl Pasta {
    fn nova(nome: &str) -> Self {
        let pasta = std::env::temp_dir().join(format!("ir-painel-{nome}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&pasta);
        std::fs::create_dir_all(&pasta).unwrap();
        Self(pasta)
    }
}

impl Drop for Pasta {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn esperar<T>(ligacao: &Ligacao, mut procura: impl FnMut(Evento) -> Option<T>) -> T {
    let fim = Instant::now() + PRAZO;
    while Instant::now() < fim {
        if let Some(evento) = ligacao.proximo(Duration::from_millis(100)) {
            if let Evento::Caiu { motivo, .. } = &evento {
                panic!("a ligação caiu: {motivo}");
            }
            if let Some(achado) = procura(evento) {
                return achado;
            }
        }
    }
    panic!("o evento não chegou");
}

async fn aviso(
    avisos: &mut mpsc::UnboundedReceiver<Aviso>,
    procura: impl Fn(&Aviso) -> bool,
) -> Aviso {
    loop {
        let aviso = tokio::time::timeout(PRAZO, avisos.recv())
            .await
            .expect("o aviso não chegou")
            .unwrap();
        if procura(&aviso) {
            return aviso;
        }
    }
}

fn quadro() -> Quadro {
    Quadro {
        maquinas: vec![Maquina {
            nome: "DESKTOP".to_owned(),
            situacao: Situacao::Aguardando,
        }],
        enlace: None,
    }
}

/// Pareia o aparelho do zero, conferindo o código nas duas telas, até o primeiro quadro.
/// Devolve a chave do aparelho, como o computador a gravou.
async fn parear(
    pasta: &Pasta,
    porta: u16,
    monitor: &Monitor,
    avisos: &mut mpsc::UnboundedReceiver<Aviso>,
) -> ir_crypto::PublicKey {
    let endereco = format!("127.0.0.1:{porta}").parse().unwrap();
    let destino = Destino::Parear {
        nome: "DESKTOP".to_owned(),
        endereco,
    };
    let ligacao = Ligacao::abrir(pasta.0.clone(), "Tablet".to_owned(), destino).unwrap();
    let digitos = tokio::task::block_in_place(|| {
        esperar(&ligacao, |e| match e {
            Evento::Codigo(d) => Some(d),
            _ => None,
        })
    });
    let Aviso::Codigo(no_computador) = aviso(avisos, |a| matches!(a, Aviso::Codigo(_))).await
    else {
        unreachable!()
    };
    assert_eq!(
        digitos, no_computador,
        "as duas telas mostram o mesmo código"
    );

    monitor.comandar(NoMonitor::Confirmar(true));
    ligacao.comandar(Comando::Confirmar(true));
    ligacao.comandar(Comando::Assinar(true));
    tokio::task::block_in_place(|| {
        esperar(&ligacao, |e| {
            matches!(e, Evento::Aberta { pareou: true, .. }).then_some(())
        });
        esperar(&ligacao, |e| matches!(e, Evento::Quadro(_)).then_some(()));
    });
    let Aviso::Pareado(chave) = aviso(avisos, |a| matches!(a, Aviso::Pareado(_))).await else {
        unreachable!()
    };
    chave
}

#[tokio::test(flavor = "multi_thread")]
async fn parear_receber_o_quadro_e_voltar_sem_codigo() {
    let pasta = Pasta::nova("ponta-a-ponta");
    let porta = porta_livre();
    let (tx, mut avisos) = mpsc::unbounded_channel();
    let monitor = Monitor::iniciar_na_porta(Identity::generate(), "DESKTOP", Vec::new(), tx, porta);
    monitor.comandar(NoMonitor::AbrirPareamento);
    monitor.publicar(quadro(), Detalhes::default());
    tokio::time::sleep(Duration::from_millis(100)).await;

    let chave = parear(&pasta, porta, &monitor, &mut avisos).await;
    let gravado = Cofre::abrir(&pasta.0).unwrap().computadores()[0].clone();
    assert_eq!(gravado.endereco, format!("127.0.0.1:{porta}"));

    // A volta: com a chave do painel conhecida, sem janela de pareamento, sem código.
    monitor.comandar(NoMonitor::Conhecidos(vec![chave]));
    let destino = Destino::Voltar(gravado);
    let volta = Ligacao::abrir(pasta.0.clone(), "Tablet".to_owned(), destino).unwrap();
    volta.comandar(Comando::Assinar(true));
    tokio::task::block_in_place(|| {
        esperar(&volta, |e| match e {
            Evento::Codigo(_) => panic!("quem volta não confere código"),
            Evento::Aberta { pareou: false, .. } => Some(()),
            _ => None,
        });
        esperar(&volta, |e| matches!(e, Evento::Quadro(_)).then_some(()));
    });

    // O interesse dito depois de aberta: os quadros seguintes trazem a cópia.
    volta.comandar(Comando::Interesse(Interesse {
        processos: vec!["docker".to_owned()],
        lista: None,
    }));
    let detalhes = Detalhes {
        copia: Some(Copia {
            enviando: true,
            nome: "fotos".to_owned(),
            feitos: 1,
            total: 2,
            por_segundo: None,
            fase: FaseDaCopia::Andando,
        }),
        processos: vec![None],
    };
    let veio = tokio::task::block_in_place(|| {
        let fim = Instant::now() + PRAZO;
        while Instant::now() < fim {
            monitor.publicar(quadro(), detalhes.clone());
            if let Some(Evento::Quadro(vista)) = volta.proximo(Duration::from_millis(100))
                && vista.copia.is_some()
            {
                return true;
            }
        }
        false
    });
    assert!(veio, "os detalhes não chegaram à vista");
}
