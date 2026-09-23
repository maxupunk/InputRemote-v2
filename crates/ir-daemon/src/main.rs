//! O executável do serviço.
//!
//! Em primeiro plano, para o teste antes da instalação como serviço/systemd: carrega
//! configuração e identidade, sobe os transportes, liga captura ou injeção conforme o papel, e
//! roda o ator central ([02, §4](../../../docs/02-arquitetura.md)).

mod actor;
mod arquivos;
mod commands;
mod config;
mod fundo;
mod ipc;

use std::sync::Arc;

use anyhow::{Context, Result};
use ir_proto::peer::{Capabilities, MachineName, PrivilegedInputLevel};
use ir_proto::screens::ScreenLayout;
use ir_session::{LocalIdentity, Role};
use tokio::sync::{mpsc, watch};
use tracing::info;
// Só o caminho sem agente (Linux) relata backend de entrada indisponível.
#[cfg(not(windows))]
use tracing::warn;

use crate::actor::{CaptureRx, Daemon, Entradas, Parts};
use ir_transporte::Endereco;

/// Ponto de entrada.
///
/// No Windows, tenta primeiro rodar como serviço do SCM; se não fomos lançados pelo SCM (execução
/// à mão, para o teste), cai para o primeiro plano. Nos demais sistemas, é sempre primeiro plano.
fn main() -> Result<()> {
    #[cfg(windows)]
    {
        // Bloqueia até o serviço parar quando o SCM nos lançou; devolve `false` fora dele.
        if ir_servico::scm::tentar_como_servico(executar_bloqueante)? {
            return Ok(());
        }
    }
    // Em primeiro plano ninguém pede parada por este canal — o processo acaba com o console —, mas
    // o emissor precisa continuar vivo: um canal sem emissor seria lido como pedido de parada.
    let (_emissor_de_parada, parada) = watch::channel(false);
    // Em primeiro plano o sistema não avisa nada por este canal; no Linux, os sinais do gancho de
    // suspensão chegam por dentro de `executar`.
    let (_emissor_do_sistema, sistema) = mpsc::unbounded_channel();
    executar_bloqueante(parada, sistema)
}

/// Monta a runtime `tokio` e roda o serviço até o fim, ou até `parada` pedir. É o caminho de
/// primeiro plano, e também o que a tarefa do serviço chama por dentro.
fn executar_bloqueante(
    parada: watch::Receiver<bool>,
    sistema: mpsc::UnboundedReceiver<ir_servico::EventoDoSistema>,
) -> Result<()> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .context("criando a runtime")?;
    runtime.block_on(executar(parada, sistema))
}

/// O corpo do serviço: carrega estado, sobe transportes, entrada e o canal de controle, e roda o ator.
async fn executar(
    parada: watch::Receiver<bool>,
    sistema: mpsc::UnboundedReceiver<ir_servico::EventoDoSistema>,
) -> Result<()> {
    // O guarda esvazia a fila do registro ao sair; soltá-lo antes perderia as últimas linhas.
    let _registro = ir_servico::registro::iniciar(&config::data_dir().join("logs"));
    #[cfg(windows)]
    if ir_sessao::como_servico() {
        ir_servico::scm::fechar_pasta_de_estado(&config::data_dir());
    }
    // Como serviço não há console: um erro de subida que não for ao registro some sem rastro.
    let resultado = subir_e_rodar(parada, sistema).await;
    if let Err(erro) = &resultado {
        tracing::error!(erro = format!("{erro:#}"), "o serviço não conseguiu subir");
    }
    resultado
}

/// Carrega o estado, sobe transportes, entrada e canais, e roda o ator até a parada.
async fn subir_e_rodar(
    parada: watch::Receiver<bool>,
    sistema: mpsc::UnboundedReceiver<ir_servico::EventoDoSistema>,
) -> Result<()> {
    let (dir, cfg, identity, role, edge) = carregar()?;
    let screen = tamanho_da_tela(&cfg);
    let maquina = ir_transporte::maquina_da_chave(&identity.public());
    let abertos = ir_transporte::abrir(cfg.port, &identity, maquina).await?;

    let (capturer, injector, capture_rx, captura) = build_io(role);
    let identidade = identidade_local(&identity);

    let canais = abrir_canais()?;
    let (de_fundo, de_fundo_rx) = tokio::sync::mpsc::unbounded_channel();
    fundo::repassar_radio_tardio(abertos.radio_tardio, de_fundo.clone());
    fundo::repassar_sistema(sistema, de_fundo.clone());
    #[cfg(target_os = "linux")]
    fundo::vigiar_a_tela(de_fundo.clone());
    let arquivos = arquivos::abrir(&cfg, &dir, &identity, &canais.avisos, &abertos.descoberta);

    let mut daemon = Daemon::new(Parts {
        session: actor::nova_sessao(role, edge, identidade.clone()),
        rede: abertos.rede,
        radio: abertos.radio,
        reabridor: Some(abertos.reabridor),
        radio_proprio: abertos.radio_proprio,
        de_fundo,
        descoberta: abertos.descoberta,
        injector,
        capturer,
        captura,
        screen,
        // `ip:porta` ou endereço de rádio: é o endereço que diz o portador.
        peer: cfg.peer_addr.as_deref().and_then(Endereco::ler),
        data_dir: dir,
        config: cfg,
        avisos: canais.avisos,
        machine: ir_ipc::Maquina(maquina.0),
        nome: ir_ipc::Nome::coagido(&ir_transporte::nome_da_maquina()),
        edge,
        agente: canais.agente,
        identidade_local: identidade,
        arquivos,
        ajudantes: canais.ajudantes,
    });

    dar_partida(&mut daemon, screen);

    daemon
        .run(Entradas {
            transportes: abertos.fatos,
            capture: capture_rx,
            confirm: fundo::spawn_stdin_reader(),
            pedidos: canais.pedidos,
            fatos: canais.fatos,
            parada,
            de_fundo: de_fundo_rx,
        })
        .await;
    Ok(())
}

/// O que a máquina guarda: diretório de estado, configuração, identidade, papel e borda.
fn carregar() -> Result<(
    std::path::PathBuf,
    config::Config,
    Arc<ir_crypto::Identity>,
    Role,
    ir_proto::screens::Edge,
)> {
    let dir = config::data_dir();
    let mut cfg = config::load_config(&dir).context("carregando configuração")?;
    let identity = Arc::new(config::load_identity(&dir).context("carregando identidade")?);
    let role = actor::papel_na_subida(&mut cfg, &dir)?;
    let edge = cfg.edge()?;
    info!(
        "InputRemote — papel {role}, impressão digital {}",
        identity.fingerprint()
    );
    Ok((dir, cfg, identity, role, edge))
}

/// Tamanho de tela: da plataforma quando ela sabe, senão da configuração.
fn tamanho_da_tela(cfg: &config::Config) -> (u32, u32) {
    ir_input::primary_screen_size().unwrap_or((cfg.screen_width, cfg.screen_height))
}

/// Dá partida no ator: as telas, a primeira tentativa de conexão e o agente.
fn dar_partida(daemon: &mut Daemon, screen: (u32, u32)) {
    feed_screens(daemon, screen);
    daemon.anunciar_radio_proprio();
    daemon.anunciar_abertura();
    daemon.verificar_economia();
    daemon.connect_if_possible();
    // O agente nasce junto com o serviço; o laço periódico só cuida de ressubi-lo se ele cair.
    daemon.garantir_agente();
}

/// Os dois canais de IPC do serviço, já no ar.
///
/// Dois transportes distintos, de propósito: a interface **não** pode pedir injeção de entrada
/// ([04, §5](../../../docs/04-seguranca.md)), e vocabulários que não se misturam são o que torna
/// essa garantia estrutural em vez de combinada.
struct Canais {
    avisos: tokio::sync::broadcast::Sender<ir_ipc::Aviso>,
    ajudantes: ipc::Ajudantes,
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
    let ajudantes = ipc::Ajudantes::default();
    let avisos = ipc::iniciar_controle(pedido_tx, ajudantes.clone())
        .context("subindo o canal de controle")?;
    // O ajudante de clipboard roda como o usuário; no Windows, quem garante que ele exista é o
    // serviço (`zelador`). No Linux, o `systemd` do usuário.
    #[cfg(windows)]
    ir_sessao::zelar_pelo_clipboard(ajudantes.clone());
    info!(endereco = %ipc::endereco_de_controle(), "canal de controle no ar");

    let (fato_tx, fatos) = mpsc::unbounded_channel();
    let agente = ipc::iniciar_agente(fato_tx).context("subindo o canal do agente")?;
    info!(endereco = %ipc::endereco_do_agente(), "canal do agente no ar");

    Ok(Canais {
        avisos,
        ajudantes,
        agente,
        pedidos,
        fatos,
    })
}

/// Captura (servidor) ou injeção (cliente), montadas conforme o papel.
type Io = (
    Option<Box<dyn ir_input::Capturer>>,
    Option<Box<dyn ir_input::Injector>>,
    CaptureRx,
    mpsc::UnboundedSender<ir_input::CaptureEvent>,
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
    // O ator guarda um emissor: é por ele que a captura começa se a máquina virar servidor depois.
    let para_o_ator = cap_tx.clone();
    #[cfg(windows)]
    {
        // No Windows quem captura e injeta é o **agente**, na sessão do usuário. O serviço não
        // toca em entrada: na sessão 0 ele não enxerga o teclado de ninguém, e um backend local
        // aqui competiria com o do agente e duplicaria cada evento.
        let _ = role;
        segurar_canal(cap_tx);
        (None, None, cap_rx, para_o_ator)
    }
    #[cfg(not(windows))]
    if role == Role::Server {
        match fundo::capturar(&cap_tx) {
            Ok(capturer) => (Some(capturer), None, cap_rx, para_o_ator),
            Err(error) => {
                warn!(%error, "captura local indisponível; a passagem de entrada aguarda o agente");
                segurar_canal(cap_tx);
                (None, None, cap_rx, para_o_ator)
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
        (None, injector, cap_rx, para_o_ator)
    }
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
        machine: ir_transporte::maquina_da_chave(&identity.public()),
        name: MachineName::coagido(&ir_transporte::nome_da_maquina()),
        capabilities: Capabilities {
            privileged_input: PrivilegedInputLevel::UnlockedOnly,
            ..Capabilities::default()
        },
    }
}

fn feed_screens(daemon: &mut Daemon, screen: (u32, u32)) {
    if let Ok(layout) = ScreenLayout::single(screen.0, screen.1) {
        daemon.definir_telas(layout);
    }
}
