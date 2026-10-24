//! O monitor de verdade, numa porta do *loopback*, com um painel de teste feito da mesma
//! [`Conversa`] que o aplicativo usa.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::time::Duration;

use ir_crypto::enlace::ConnectMode;
use ir_crypto::{Identity, PublicKey};
use ir_monitor::mensagem::{codificar, decodificar};
use ir_monitor::{
    Apresentacao, Conversa, Detalhes, DoPainel, Evento, Interesse, Maquina, ParaPainel, Quadro,
    Situacao,
};
use ir_monitor_rede::{Aviso, Comando, Monitor};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::mpsc;

const PRAZO: Duration = Duration::from_secs(5);

/// Uma porta livre agora.
fn porta_livre() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

/// O painel de teste.
struct Painel {
    socket: TcpStream,
    conversa: Conversa,
}

impl Painel {
    async fn discar(porta: u16, identidade: &Identity, modo: ConnectMode) -> Self {
        let mut socket = conectar(porta).await;
        let (conversa, primeiros) = Conversa::discar(identidade, modo).unwrap();
        socket.write_all(&primeiros).await.unwrap();
        Self { socket, conversa }
    }

    /// Lê até aparecer um evento que `procura` aceite, mandando o que a conversa pedir.
    async fn ate<T>(&mut self, mut procura: impl FnMut(&Evento) -> Option<T>) -> T {
        let mut buffer = [0u8; 4096];
        loop {
            let lidos = tokio::time::timeout(PRAZO, self.socket.read(&mut buffer))
                .await
                .expect("o computador não respondeu a tempo")
                .unwrap();
            assert!(lidos > 0, "o computador fechou a conexão");
            let eventos = self.conversa.alimentar(&buffer[..lidos]).unwrap();
            let mut achado = None;
            for evento in &eventos {
                if let Evento::Enviar(bytes) = evento {
                    self.socket.write_all(bytes).await.unwrap();
                }
                if achado.is_none() {
                    achado = procura(evento);
                }
            }
            if let Some(achado) = achado {
                return achado;
            }
        }
    }

    async fn mandar(&mut self, mensagem: &DoPainel) {
        let bytes = self.conversa.enviar(&codificar(mensagem).unwrap()).unwrap();
        self.socket.write_all(&bytes).await.unwrap();
    }

    async fn proxima(&mut self) -> ParaPainel {
        self.ate(|evento| match evento {
            Evento::Mensagem(bytes) => Some(decodificar::<ParaPainel>(bytes).unwrap()),
            _ => None,
        })
        .await
    }

    /// As próximas `quantas` mensagens — que podem chegar juntas numa leitura só.
    async fn mensagens(&mut self, quantas: usize) -> Vec<ParaPainel> {
        let mut buffer = [0u8; 4096];
        let mut recebidas = Vec::new();
        while recebidas.len() < quantas {
            let lidos = tokio::time::timeout(PRAZO, self.socket.read(&mut buffer))
                .await
                .expect("o computador não respondeu a tempo")
                .unwrap();
            for evento in self.conversa.alimentar(&buffer[..lidos]).unwrap() {
                if let Evento::Mensagem(bytes) = evento {
                    recebidas.push(decodificar::<ParaPainel>(&bytes).unwrap());
                }
            }
        }
        recebidas
    }

    /// Se o computador fecha a conexão sem abri-la.
    async fn foi_recusado(&mut self) -> bool {
        let mut buffer = [0u8; 4096];
        loop {
            match tokio::time::timeout(PRAZO, self.socket.read(&mut buffer)).await {
                Ok(Ok(0) | Err(_)) => return true,
                Ok(Ok(lidos)) => match self.conversa.alimentar(&buffer[..lidos]) {
                    Ok(eventos) if eventos.iter().any(|e| matches!(e, Evento::Aberta { .. })) => {
                        return false;
                    }
                    Ok(_) => {}
                    Err(_) => return true,
                },
                Err(_) => return false,
            }
        }
    }
}

async fn conectar(porta: u16) -> TcpStream {
    // A central abre a porta logo depois do comando; algumas tentativas cobrem a corrida.
    for _ in 0..50 {
        if let Ok(socket) = TcpStream::connect(("127.0.0.1", porta)).await {
            return socket;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("a porta do painel não abriu");
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

/// Pareia um painel novo do começo ao fim, conferindo o código nas duas telas.
async fn parear(
    monitor: &Monitor,
    avisos: &mut mpsc::UnboundedReceiver<Aviso>,
    porta: u16,
) -> (Painel, Identity) {
    monitor.comandar(Comando::AbrirPareamento);
    let painel_id = Identity::generate();
    let mut painel = Painel::discar(porta, &painel_id, ConnectMode::Pair).await;
    let no_painel = painel
        .ate(|e| match e {
            Evento::Codigo { digitos, .. } => Some(*digitos),
            _ => None,
        })
        .await;
    let Aviso::Codigo(no_computador) = aviso(avisos, |a| matches!(a, Aviso::Codigo(_))).await
    else {
        unreachable!()
    };
    assert_eq!(
        no_painel, no_computador,
        "as duas telas mostram o mesmo código"
    );

    monitor.comandar(Comando::Confirmar(true));
    for evento in painel.conversa.confirmar(true).unwrap() {
        if let Evento::Enviar(bytes) = evento {
            painel.socket.write_all(&bytes).await.unwrap();
        }
    }
    // O painel só abre ao ler a confirmação do computador: as duas são necessárias.
    painel
        .ate(|e| matches!(e, Evento::Aberta { pareou: true, .. }).then_some(()))
        .await;
    let pareado = aviso(avisos, |a| matches!(a, Aviso::Pareado(_))).await;
    assert_eq!(pareado, Aviso::Pareado(painel_id.public()));
    (painel, painel_id)
}

#[tokio::test]
async fn parear_apresentar_assinar_e_receber_o_quadro() {
    let porta = porta_livre();
    let (tx, mut avisos) = mpsc::unbounded_channel();
    let monitor = Monitor::iniciar_na_porta(Identity::generate(), "DESKTOP", Vec::new(), tx, porta);
    let (mut painel, painel_id) = parear(&monitor, &mut avisos, porta).await;

    painel
        .mandar(&DoPainel::Ola(Apresentacao::desta_ponta("Tablet")))
        .await;
    let ola = painel.proxima().await;
    assert_eq!(ola, ParaPainel::Ola(Apresentacao::desta_ponta("DESKTOP")));
    let apresentou = aviso(&mut avisos, |a| matches!(a, Aviso::Apresentou { .. })).await;
    assert_eq!(
        apresentou,
        Aviso::Apresentou {
            chave: painel_id.public(),
            nome: "Tablet".to_owned()
        }
    );

    monitor.publicar(quadro(), Detalhes::default());
    painel.mandar(&DoPainel::Assinar(true)).await;
    assert_eq!(
        aviso(&mut avisos, |a| matches!(a, Aviso::Assinantes(_))).await,
        Aviso::Assinantes(1)
    );
    assert_eq!(painel.proxima().await, ParaPainel::Quadro(quadro()));

    painel.mandar(&DoPainel::Assinar(false)).await;
    assert_eq!(
        aviso(&mut avisos, |a| matches!(a, Aviso::Assinantes(_))).await,
        Aviso::Assinantes(0),
        "sem assinante o serviço para de coletar"
    );
}

/// Os detalhes só vão a quem os pediu, e o pedido vira o que medir em cada computador.
#[tokio::test]
async fn quem_diz_interesse_recebe_os_detalhes() {
    let porta = porta_livre();
    let (tx, mut avisos) = mpsc::unbounded_channel();
    let monitor = Monitor::iniciar_na_porta(Identity::generate(), "DESKTOP", Vec::new(), tx, porta);
    let (mut painel, _) = parear(&monitor, &mut avisos, porta).await;

    let detalhes = Detalhes {
        copia: None,
        processos: vec![None],
    };
    monitor.publicar(quadro(), detalhes.clone());
    painel
        .mandar(&DoPainel::Interesse(Interesse {
            processos: vec!["docker".to_owned()],
            lista: Some(0),
        }))
        .await;
    painel.mandar(&DoPainel::Assinar(true)).await;
    let Aviso::Interesse([daqui, par]) =
        aviso(&mut avisos, |a| matches!(a, Aviso::Interesse(_))).await
    else {
        unreachable!()
    };
    assert_eq!(daqui.nomes, vec!["docker"]);
    assert!(daqui.lista && !par.lista, "a lista só de onde foi aberta");
    assert_eq!(
        painel.mensagens(2).await,
        vec![ParaPainel::Quadro(quadro()), ParaPainel::Detalhes(detalhes)]
    );

    painel.mandar(&DoPainel::Assinar(false)).await;
    let Aviso::Interesse(depois) = aviso(&mut avisos, |a| matches!(a, Aviso::Interesse(_))).await
    else {
        unreachable!()
    };
    assert!(
        depois.iter().all(ir_monitor::InteresseEmProcessos::vazio),
        "sem assinante, nada de processos"
    );
}

#[tokio::test]
async fn um_painel_conhecido_volta_e_um_estranho_nao() {
    let porta = porta_livre();
    let (tx, _avisos) = mpsc::unbounded_channel();
    let computador = Identity::generate();
    let conhecido = Identity::generate();
    let _monitor = Monitor::iniciar_na_porta(
        computador.clone(),
        "DESKTOP",
        vec![conhecido.public()],
        tx,
        porta,
    );
    let volta = ConnectMode::Reconnect(computador.public());

    let mut estranho = Painel::discar(porta, &Identity::generate(), volta).await;
    assert!(
        estranho.foi_recusado().await,
        "um painel não pareado não volta"
    );

    let mut painel = Painel::discar(porta, &conhecido, volta).await;
    let par: PublicKey = painel
        .ate(|e| match e {
            Evento::Aberta { par, .. } => Some(*par),
            _ => None,
        })
        .await;
    assert_eq!(par, computador.public());
}

#[tokio::test]
async fn sem_janela_um_painel_novo_nao_pareia() {
    let porta = porta_livre();
    let (tx, _avisos) = mpsc::unbounded_channel();
    // Com um conhecido a porta abre, mas a janela de pareamento está fechada.
    let monitor = Monitor::iniciar_na_porta(
        Identity::generate(),
        "DESKTOP",
        vec![Identity::generate().public()],
        tx,
        porta,
    );
    let mut painel = Painel::discar(porta, &Identity::generate(), ConnectMode::Pair).await;
    assert!(painel.foi_recusado().await);
    drop(monitor);
}

#[tokio::test]
async fn esquecer_o_painel_derruba_a_conexao_dele() {
    let porta = porta_livre();
    let (tx, mut avisos) = mpsc::unbounded_channel();
    let computador = Identity::generate();
    let conhecido = Identity::generate();
    let monitor = Monitor::iniciar_na_porta(
        computador.clone(),
        "DESKTOP",
        vec![conhecido.public()],
        tx,
        porta,
    );
    let mut painel = Painel::discar(
        porta,
        &conhecido,
        ConnectMode::Reconnect(computador.public()),
    )
    .await;
    painel
        .ate(|e| matches!(e, Evento::Aberta { .. }).then_some(()))
        .await;
    aviso(
        &mut avisos,
        |a| matches!(a, Aviso::Conectados(c) if !c.is_empty()),
    )
    .await;

    monitor.comandar(Comando::Conhecidos(Vec::new()));
    aviso(
        &mut avisos,
        |a| matches!(a, Aviso::Conectados(c) if c.is_empty()),
    )
    .await;
    let mut buffer = [0u8; 64];
    let lido = tokio::time::timeout(PRAZO, painel.socket.read(&mut buffer)).await;
    assert!(
        matches!(lido, Ok(Ok(0) | Err(_))),
        "a conexão do esquecido fecha"
    );
}
