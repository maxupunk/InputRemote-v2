//! Como o ator nasce, e por onde ele é alimentado.
//!
//! Duas coisas do mesmo assunto: [`Parts`], o que o [`Daemon`] precisa para existir, e
//! [`Entradas`], as origens de evento que o mantêm vivo. Ficam fora do laço central por tamanho
//! — o laço é o que se lê para entender o serviço, e uma lista de campos no meio dele só atrapalha.

use std::net::SocketAddr;
use std::time::Instant;

use ir_input::{Capturer, Injector};
use ir_ipc::{Aviso, ComandoDoAgente, FatoDoAgente, Maquina, Nome};
use ir_net::{NetCommand, NetEvent};
use ir_proto::screens::Edge;
use ir_session::{CommandBatch, LocalIdentity, Phase, Session};
use tokio::sync::broadcast;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};

use super::{CaptureRx, Daemon};
use crate::config::Config;
use crate::ipc::PedidoRecebido;

/// Tudo que alimenta o ator, num valor só.
///
/// Juntos e não soltos porque são a mesma coisa vista de cinco lados: as origens de evento do
/// serviço. Passá-los um a um transformaria cada canal novo numa mudança de assinatura.
pub(crate) struct Entradas {
    /// Eventos do endpoint de rede.
    pub(crate) net_events: UnboundedReceiver<NetEvent>,
    /// Entrada capturada localmente (só onde não há agente).
    pub(crate) capture: CaptureRx,
    /// Confirmação de pareamento vinda do terminal.
    pub(crate) confirm: UnboundedReceiver<String>,
    /// Pedidos da interface.
    pub(crate) pedidos: UnboundedReceiver<PedidoRecebido>,
    /// Fatos do agente.
    pub(crate) fatos: UnboundedReceiver<FatoDoAgente>,
    /// O pedido de parada do serviço.
    pub(crate) parada: tokio::sync::watch::Receiver<bool>,
}

/// O que o ator precisa para nascer.
pub(crate) struct Parts {
    /// A sessão já configurada.
    pub(crate) session: Session,
    /// Canal de comandos para o endpoint de rede.
    pub(crate) net: UnboundedSender<NetCommand>,
    /// Injetor (cliente) ou nada.
    pub(crate) injector: Option<Box<dyn Injector>>,
    /// Capturador (servidor) ou nada.
    pub(crate) capturer: Option<Box<dyn Capturer>>,
    /// Tamanho da tela local, em pixels.
    pub(crate) screen: (u32, u32),
    /// Endereço do par, se conhecido.
    pub(crate) peer_addr: Option<SocketAddr>,
    /// Diretório de estado.
    pub(crate) data_dir: std::path::PathBuf,
    /// A configuração corrente.
    pub(crate) config: Config,
    /// Emissor de avisos para as interfaces.
    pub(crate) avisos: broadcast::Sender<Aviso>,
    /// Esta máquina, já no vocabulário da interface.
    pub(crate) machine: Maquina,
    /// O nome desta máquina.
    pub(crate) nome: Nome,
    /// A borda que dá para o par.
    pub(crate) edge: Edge,
    /// Emissor de comandos para o agente.
    pub(crate) agente: broadcast::Sender<ComandoDoAgente>,
    /// Quem esta máquina é, guardada para recriar a sessão numa troca de papel ou de borda.
    pub(crate) identidade_local: LocalIdentity,
}

impl Daemon {
    /// Monta o ator.
    #[must_use]
    pub(crate) fn new(parts: Parts) -> Self {
        Self {
            session: parts.session,
            out: CommandBatch::with_capacity(32),
            start: Instant::now(),
            net: parts.net,
            injector: parts.injector,
            capturer: parts.capturer,
            screen: parts.screen,
            peer_addr: parts.peer_addr,
            data_dir: parts.data_dir,
            config: parts.config,
            pending_peer: None,
            pareamento_desde: None,
            seed_pointer: true,
            linked: false,
            ticks: 0,
            avisos: parts.avisos,
            machine: parts.machine,
            nome: parts.nome,
            edge: parts.edge,
            last_phase: Phase::Offline,
            agente: parts.agente,
            agente_pronto: false,
            identidade_local: parts.identidade_local,
            ultimo_arranjo: None,
        }
    }
}
