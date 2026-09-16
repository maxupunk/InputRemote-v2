//! Como o ator nasce, e por onde ele é alimentado.
//!
//! Duas coisas do mesmo assunto: [`Parts`], o que o [`Daemon`] precisa para existir, e
//! [`Entradas`], as origens de evento que o mantêm vivo. Ficam fora do laço central por tamanho
//! — o laço é o que se lê para entender o serviço, e uma lista de campos no meio dele só atrapalha.

use std::sync::Arc;
use std::time::Instant;

use ir_input::{Capturer, Injector};
use ir_ipc::{Aviso, ComandoDoAgente, FatoDoAgente, Maquina, Nome};
use ir_proto::screens::Edge;
use ir_session::{CommandBatch, LocalIdentity, Phase, Session};
use tokio::sync::broadcast;
use tokio::sync::mpsc::UnboundedReceiver;

use super::{CaptureRx, Daemon};
use crate::config::Config;
use crate::ipc::PedidoRecebido;
use ir_transporte::{Endereco, Transporte};

/// Tudo que alimenta o ator, num valor só.
///
/// Juntos e não soltos porque são a mesma coisa vista de cinco lados: as origens de evento do
/// serviço. Passá-los um a um transformaria cada canal novo numa mudança de assinatura.
pub(crate) struct Entradas {
    /// O que os transportes relatam — **os dois pelo mesmo canal**.
    ///
    /// Um canal só, e não um por portador: cada fato já diz por onde veio, e é o que permite ao
    /// laço central ter um caminho de código para os dois. Um canal por transporte obrigaria o
    /// laço a crescer a cada portador novo.
    pub(crate) transportes: UnboundedReceiver<ir_transporte::Fato>,
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
    /// O transporte de rede. Sempre existe: um socket UDP local sempre vincula.
    pub(crate) rede: Arc<dyn Transporte>,
    /// O transporte de rádio, quando há rádio.
    ///
    /// `None` é um estado normal e esperado — sem adaptador, desligado, ou com o canal do
    /// produto ocupado. É a ausência que faz a sessão degradar para a rede, **com o motivo
    /// visível na tela**, em vez de o produto insistir num rádio que não existe.
    pub(crate) radio: Option<Arc<dyn Transporte>>,
    /// Injetor (cliente) ou nada.
    pub(crate) injector: Option<Box<dyn Injector>>,
    /// Capturador (servidor) ou nada.
    pub(crate) capturer: Option<Box<dyn Capturer>>,
    /// Tamanho da tela local, em pixels.
    pub(crate) screen: (u32, u32),
    /// Onde o par foi visto pela última vez, se em algum lugar.
    pub(crate) peer: Option<Endereco>,
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
            rede: parts.rede,
            radio: parts.radio,
            injector: parts.injector,
            capturer: parts.capturer,
            screen: parts.screen,
            peer: parts.peer,
            data_dir: parts.data_dir,
            config: parts.config,
            pending_peer: None,
            pareamento: None,
            // Ninguém fixou nada até a interface pedir: a escolha começa automática.
            portador_fixado: None,
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
