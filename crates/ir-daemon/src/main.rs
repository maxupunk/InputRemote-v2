//! O executável do serviço.
//!
//! Em primeiro plano, para o teste antes da instalação como serviço/systemd: carrega
//! configuração e identidade, sobe o endpoint UDP, liga captura ou injeção conforme o papel, e
//! roda o ator central ([02, §4](../../../docs/02-arquitetura.md)).

mod actor;
mod commands;
mod config;

use std::io::BufRead;
use std::sync::Arc;

use anyhow::{Context, Result};
use ir_net::{Endpoint, bind};
use ir_proto::peer::{Capabilities, MachineName, PrivilegedInputLevel};
use ir_proto::screens::ScreenLayout;
use ir_session::{Input, LocalIdentity, Role, Session, SessionConfig, Timestamp};
use tokio::sync::mpsc;
use tracing::info;

use crate::actor::{CaptureRx, Daemon, Parts};

#[tokio::main]
async fn main() -> Result<()> {
    init_tracing();

    let dir = config::data_dir();
    let cfg = config::load_config(&dir).context("carregando configuração")?;
    let identity = Arc::new(config::load_identity(&dir).context("carregando identidade")?);
    let role = cfg.session_role()?;
    let edge = cfg.edge()?;
    info!(
        "InputRemote daemon — papel {role}, impressão digital {}",
        identity.fingerprint()
    );

    // Tamanho de tela: da plataforma quando ela sabe, senão da configuração.
    let screen = ir_input::primary_screen_size().unwrap_or((cfg.screen_width, cfg.screen_height));

    let socket = bind(
        format!("0.0.0.0:{}", cfg.port)
            .parse()
            .context("porta inválida")?,
    )
    .await
    .context("vinculando o socket UDP")?;
    info!(local = %socket.local_addr().context("endereço local")?, "escutando UDP");
    let net = Endpoint::spawn(Arc::clone(&socket), Arc::clone(&identity));

    let (capturer, injector, capture_rx) = build_io(role)?;
    let session = build_session(role, edge, &identity, &cfg);
    let peer_addr = cfg.peer_addr.as_deref().and_then(|a| a.parse().ok());

    let mut daemon = Daemon::new(Parts {
        session,
        net: net.commands.clone(),
        injector,
        capturer,
        screen,
        peer_addr,
        data_dir: dir,
        config: cfg,
    });

    feed_screens(&mut daemon, screen);
    daemon.connect_if_possible();
    let confirm_rx = spawn_stdin_reader();

    daemon.run(net.events, capture_rx, confirm_rx).await;
    Ok(())
}

/// Configura o `tracing`, com nível de `RUST_LOG` ou `info` por padrão.
fn init_tracing() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();
}

/// A ponte da confirmação de pareamento: lê linhas do stdin numa thread própria.
fn spawn_stdin_reader() -> mpsc::UnboundedReceiver<String> {
    let (tx, rx) = mpsc::unbounded_channel();
    std::thread::spawn(move || {
        let stdin = std::io::stdin();
        for line in stdin.lock().lines().map_while(Result::ok) {
            if tx.send(line).is_err() {
                break;
            }
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
fn build_io(role: Role) -> Result<Io> {
    let (cap_tx, cap_rx) = mpsc::unbounded_channel();
    if role == Role::Server {
        // Ponte da captura (thread std) para o canal do ator (tokio).
        let (std_tx, std_rx) = std::sync::mpsc::channel();
        let capturer = ir_input::start_capture(std_tx).context("instalando a captura")?;
        std::thread::spawn(move || {
            while let Ok(event) = std_rx.recv() {
                if cap_tx.send(event).is_err() {
                    break;
                }
            }
        });
        Ok((Some(capturer), None, cap_rx))
    } else {
        let injector = ir_input::open_injector().context("abrindo o injetor")?;
        Ok((None, Some(injector), cap_rx))
    }
}

/// Monta a sessão a partir da configuração e da identidade.
fn build_session(
    role: Role,
    edge: ir_proto::screens::Edge,
    identity: &ir_crypto::Identity,
    _cfg: &config::Config,
) -> Session {
    let capabilities = Capabilities {
        privileged_input: PrivilegedInputLevel::UnlockedOnly,
        ..Capabilities::default()
    };
    let name = MachineName::coagido(&hostname());
    let local = LocalIdentity {
        machine: machine_id_from(identity),
        name,
        capabilities,
    };
    let config = if role == Role::Server {
        SessionConfig::server(edge)
    } else {
        SessionConfig::client(edge)
    };
    Session::new(config, local)
}

/// Deriva um id de máquina estável dos primeiros bytes da chave pública.
fn machine_id_from(identity: &ir_crypto::Identity) -> ir_proto::ids::MachineId {
    ir_proto::ids::MachineId(identity.public().0[..16].try_into().unwrap_or([0u8; 16]))
}

fn feed_screens(daemon: &mut Daemon, screen: (u32, u32)) {
    if let Ok(layout) = ScreenLayout::single(screen.0, screen.1) {
        let now = Timestamp::from_micros(0);
        daemon
            .session
            .step(now, Input::LocalScreens(layout), &mut daemon.out);
        daemon.apply_commands();
    }
}

fn hostname() -> String {
    std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .unwrap_or_else(|_| "computador".to_owned())
}
