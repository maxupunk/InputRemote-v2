//! O executável do serviço.
//!
//! Em primeiro plano, para o teste antes da instalação como serviço/systemd: carrega
//! configuração e identidade, sobe o endpoint UDP, liga captura ou injeção conforme o papel, e
//! roda o ator central ([02, §4](../../../docs/02-arquitetura.md)).

mod actor;
mod commands;
mod config;
mod ipc;
#[cfg(windows)]
mod lancador;
#[cfg(windows)]
mod service;

use std::io::BufRead;
use std::sync::Arc;

use anyhow::{Context, Result};
use ir_net::{Endpoint, bind};
use ir_proto::peer::{Capabilities, MachineName, PrivilegedInputLevel};
use ir_proto::screens::ScreenLayout;
use ir_session::{LocalIdentity, Role};
use tokio::sync::{mpsc, watch};
use tracing::info;
// Só o caminho sem agente (Linux) relata backend de entrada indisponível.
#[cfg(not(windows))]
use tracing::warn;

use crate::actor::{CaptureRx, Daemon, Entradas, Parts};

/// Ponto de entrada.
///
/// No Windows, tenta primeiro rodar como serviço do SCM; se não fomos lançados pelo SCM (execução
/// à mão, para o teste), cai para o primeiro plano. Nos demais sistemas, é sempre primeiro plano.
fn main() -> Result<()> {
    #[cfg(windows)]
    {
        // Bloqueia até o serviço parar quando o SCM nos lançou; devolve `false` fora dele.
        if service::tentar_como_servico()? {
            return Ok(());
        }
    }
    // Em primeiro plano ninguém pede parada por este canal — o processo acaba com o console —, mas
    // o emissor precisa continuar vivo: um canal sem emissor seria lido como pedido de parada.
    let (_emissor_de_parada, parada) = watch::channel(false);
    executar_bloqueante(parada)
}

/// Monta a runtime `tokio` e roda o serviço até o fim, ou até `parada` pedir. É o caminho de
/// primeiro plano, e também o que a tarefa do serviço chama por dentro.
pub(crate) fn executar_bloqueante(parada: watch::Receiver<bool>) -> Result<()> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .context("criando a runtime")?;
    runtime.block_on(executar(parada))
}

/// O corpo do serviço: carrega estado, sobe rede, entrada e o canal de controle, e roda o ator.
async fn executar(parada: watch::Receiver<bool>) -> Result<()> {
    // O guarda esvazia a fila do registro ao sair; soltá-lo antes perderia as últimas linhas.
    let _registro = init_tracing();

    let dir = config::data_dir();
    let mut cfg = config::load_config(&dir).context("carregando configuração")?;
    let identity = Arc::new(config::load_identity(&dir).context("carregando identidade")?);
    let role = actor::papel_na_subida(&mut cfg, &dir)?;
    let edge = cfg.edge()?;
    info!(
        "InputRemote daemon — papel {role}, impressão digital {}",
        identity.fingerprint()
    );

    // Tamanho de tela: da plataforma quando ela sabe, senão da configuração.
    let screen = ir_input::primary_screen_size().unwrap_or((cfg.screen_width, cfg.screen_height));

    let socket = abrir_socket(cfg.port).await?;
    let net = Endpoint::spawn(Arc::clone(&socket), Arc::clone(&identity));

    let (capturer, injector, capture_rx) = build_io(role);
    let identidade = identidade_local(&identity);
    let peer_addr = cfg.peer_addr.as_deref().and_then(|a| a.parse().ok());

    let canais = abrir_canais()?;

    let mut daemon = Daemon::new(Parts {
        session: actor::nova_sessao(role, edge, identidade.clone()),
        net: net.commands.clone(),
        injector,
        capturer,
        screen,
        peer_addr,
        data_dir: dir,
        config: cfg,
        avisos: canais.avisos,
        machine: ir_ipc::Maquina(machine_id_from(&identity).0),
        nome: ir_ipc::Nome::coagido(&hostname()),
        edge,
        agente: canais.agente,
        identidade_local: identidade,
    });

    feed_screens(&mut daemon, screen);
    daemon.connect_if_possible();
    // O agente nasce junto com o serviço; o laço periódico só cuida de ressubi-lo se ele cair.
    daemon.garantir_agente();

    daemon
        .run(Entradas {
            net_events: net.events,
            capture: capture_rx,
            confirm: spawn_stdin_reader(),
            pedidos: canais.pedidos,
            fatos: canais.fatos,
            parada,
        })
        .await;
    Ok(())
}

/// Os dois canais de IPC do serviço, já no ar.
///
/// Dois transportes distintos, de propósito: a interface **não** pode pedir injeção de entrada
/// ([04, §5](../../../docs/04-seguranca.md)), e vocabulários que não se misturam são o que torna
/// essa garantia estrutural em vez de combinada.
struct Canais {
    avisos: tokio::sync::broadcast::Sender<ir_ipc::Aviso>,
    agente: tokio::sync::broadcast::Sender<ir_ipc::ComandoDoAgente>,
    pedidos: mpsc::UnboundedReceiver<ipc::PedidoRecebido>,
    fatos: mpsc::UnboundedReceiver<ir_ipc::FatoDoAgente>,
}

/// Sobe os dois canais antes do ator, para os emissores já existirem quando ele nascer.
///
/// O canal do agente sobe mesmo no Linux, onde nenhum agente conecta: manter o receptor de
/// fatos vivo é o que impede o laço do ator de girar recebendo `None` sem parar.
fn abrir_canais() -> Result<Canais> {
    let (pedido_tx, pedidos) = mpsc::unbounded_channel();
    let avisos = ipc::iniciar_controle(pedido_tx).context("subindo o canal de controle")?;
    info!(endereco = %ipc::endereco_de_controle(), "canal de controle no ar");

    let (fato_tx, fatos) = mpsc::unbounded_channel();
    let agente = ipc::iniciar_agente(fato_tx).context("subindo o canal do agente")?;
    info!(endereco = %ipc::endereco_do_agente(), "canal do agente no ar");

    Ok(Canais {
        avisos,
        agente,
        pedidos,
        fatos,
    })
}

/// Quantos arquivos de registro diários o serviço do Windows guarda.
#[cfg(windows)]
const DIAS_DE_REGISTRO: usize = 7;

/// Configura o registro, com nível de `RUST_LOG` ou `info` por padrão, sem bloquear quem registra.
///
/// O escritor é de fila: uma linha nunca espera o disco ou o console, porque quem registra pode ser
/// o laço da sessão, que bate a cada 5 ms. O guarda devolvido esvazia a fila ao sair, e precisa
/// viver até o fim.
fn init_tracing() -> tracing_appender::non_blocking::WorkerGuard {
    let (destino, terminal) = destino_do_registro();
    let (escritor, guarda) = tracing_appender::non_blocking(destino);
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .with_writer(escritor)
        // Cor só num terminal de verdade: num arquivo ou no `journald`, os códigos de cor viram
        // lixo no meio de cada linha.
        .with_ansi(terminal)
        .init();
    guarda
}

/// Para onde vai o registro, e se o destino é um terminal.
///
/// Como serviço do Windows, para `%ProgramData%\InputRemote\logs`: ninguém lê a saída padrão de um
/// serviço, e um serviço que falha sem deixar rastro não tem como ser diagnosticado. Em primeiro
/// plano, e no Linux — onde o `journald` já guarda a saída do serviço —, para a saída padrão.
fn destino_do_registro() -> (Box<dyn std::io::Write + Send>, bool) {
    #[cfg(windows)]
    {
        if lancador::como_servico()
            && let Some(arquivo) = arquivo_de_registro()
        {
            return (Box::new(arquivo), false);
        }
    }
    let terminal = std::io::IsTerminal::is_terminal(&std::io::stdout());
    (Box::new(std::io::stdout()), terminal)
}

/// O arquivo de registro do serviço do Windows, com um arquivo por dia e os mais velhos apagados.
#[cfg(windows)]
fn arquivo_de_registro() -> Option<tracing_appender::rolling::RollingFileAppender> {
    use tracing_appender::rolling::{RollingFileAppender, Rotation};
    RollingFileAppender::builder()
        .rotation(Rotation::DAILY)
        .filename_prefix("inputremote")
        .filename_suffix("log")
        .max_log_files(DIAS_DE_REGISTRO)
        .build(config::data_dir().join("logs"))
        .ok()
}

/// Vincula o socket UDP local na porta dada e registra o endereço.
async fn abrir_socket(port: u16) -> Result<Arc<tokio::net::UdpSocket>> {
    let socket = bind(
        format!("0.0.0.0:{port}")
            .parse()
            .context("porta inválida")?,
    )
    .await
    .context("vinculando o socket UDP")?;
    info!(local = %socket.local_addr().context("endereço local")?, "escutando UDP");
    Ok(socket)
}

/// A ponte da confirmação de pareamento: lê linhas do stdin numa thread própria.
fn spawn_stdin_reader() -> mpsc::UnboundedReceiver<String> {
    let (tx, rx) = mpsc::unbounded_channel();
    std::thread::spawn(move || {
        let stdin = std::io::stdin();
        for line in stdin.lock().lines().map_while(Result::ok) {
            if tx.send(line).is_err() {
                return;
            }
        }
        // Fim do stdin (redirecionado de um arquivo, por exemplo): segura o emissor para o canal
        // não fechar, senão o laço do ator giraria recebendo `None` sem parar.
        loop {
            std::thread::park();
        }
    });
    rx
}

/// Captura (servidor) ou injeção (cliente), montadas conforme o papel.
type Io = (
    Option<Box<dyn ir_input::Capturer>>,
    Option<Box<dyn ir_input::Injector>>,
    CaptureRx,
);

/// Liga o backend de entrada conforme o papel.
///
/// A falha de um backend **não** derruba o serviço. Rodando como serviço na sessão 0, a captura e
/// a injeção diretas não alcançam a sessão do usuário — é para isso que existe o agente ([05,
/// §5](../../../docs/05-windows.md)). Enquanto o agente não entra, o serviço fica de pé mesmo
/// assim: o canal de controle funciona, o pareamento pela interface funciona, e só a passagem de
/// teclado e mouse é que espera o agente. Um serviço que caísse por não capturar nada seria o
/// pior dos mundos — nem sobe, nem diz por quê.
fn build_io(role: Role) -> Io {
    let (cap_tx, cap_rx) = mpsc::unbounded_channel();
    #[cfg(windows)]
    {
        // No Windows quem captura e injeta é o **agente**, na sessão do usuário. O serviço não
        // toca em entrada: na sessão 0 ele não enxerga o teclado de ninguém, e um backend local
        // aqui competiria com o do agente e duplicaria cada evento.
        let _ = role;
        segurar_canal(cap_tx);
        (None, None, cap_rx)
    }
    #[cfg(not(windows))]
    if role == Role::Server {
        // Ponte da captura (thread std) para o canal do ator (tokio).
        let (std_tx, std_rx) = std::sync::mpsc::channel();
        match ir_input::start_capture(std_tx) {
            Ok(capturer) => {
                bridge_captura(std_rx, cap_tx);
                (Some(capturer), None, cap_rx)
            }
            Err(error) => {
                warn!(%error, "captura local indisponível; a passagem de entrada aguarda o agente");
                segurar_canal(cap_tx);
                (None, None, cap_rx)
            }
        }
    } else {
        let injector = match ir_input::open_injector() {
            Ok(injector) => Some(injector),
            Err(error) => {
                warn!(%error, "injeção local indisponível; a passagem de entrada aguarda o agente");
                None
            }
        };
        segurar_canal(cap_tx);
        (None, injector, cap_rx)
    }
}

/// Ponte da captura: repassa cada evento da thread `std` da captura para o canal do ator.
#[cfg(not(windows))]
fn bridge_captura(
    std_rx: std::sync::mpsc::Receiver<ir_input::CaptureEvent>,
    cap_tx: mpsc::UnboundedSender<ir_input::CaptureEvent>,
) {
    std::thread::spawn(move || {
        while let Ok(event) = std_rx.recv() {
            if cap_tx.send(event).is_err() {
                break;
            }
        }
    });
}

/// Segura o emissor de captura numa tarefa, para o canal não fechar quando ninguém captura.
///
/// Um `recv` num canal fechado volta na hora, e o laço do ator giraria sem parar; manter um
/// emissor vivo evita isso.
fn segurar_canal(cap_tx: mpsc::UnboundedSender<ir_input::CaptureEvent>) {
    tokio::spawn(async move {
        let _hold = cap_tx;
        std::future::pending::<()>().await;
    });
}

/// Quem esta máquina é, do ponto de vista do protocolo.
///
/// O ator guarda esta identidade, e não só a usa na subida: trocar papel ou borda recria a sessão,
/// e a nova precisa nascer com a mesma.
fn identidade_local(identity: &ir_crypto::Identity) -> LocalIdentity {
    LocalIdentity {
        machine: machine_id_from(identity),
        name: MachineName::coagido(&hostname()),
        capabilities: Capabilities {
            privileged_input: PrivilegedInputLevel::UnlockedOnly,
            ..Capabilities::default()
        },
    }
}

/// Deriva um id de máquina estável dos primeiros bytes da chave pública.
fn machine_id_from(identity: &ir_crypto::Identity) -> ir_proto::ids::MachineId {
    ir_proto::ids::MachineId(identity.public().0[..16].try_into().unwrap_or([0u8; 16]))
}

fn feed_screens(daemon: &mut Daemon, screen: (u32, u32)) {
    if let Ok(layout) = ScreenLayout::single(screen.0, screen.1) {
        daemon.definir_telas(layout);
    }
}

fn hostname() -> String {
    std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .unwrap_or_else(|_| "computador".to_owned())
}
