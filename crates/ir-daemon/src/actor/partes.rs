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
    /// O que as tarefas de fundo devolvem ([`DeFundo`]).
    pub(crate) de_fundo: UnboundedReceiver<DeFundo>,
}

/// O que as tarefas de fundo do ator devolvem a ele.
///
/// Um canal só para todas: cada uma roda fora do ator porque bloqueia ou demora, e todas terminam
/// num evento que o ator trata na vez dele. Um canal por tarefa faria o laço crescer a cada uma.
#[derive(Debug)]
pub(crate) enum DeFundo {
    /// A descoberta achou o par na rede, para a rota dupla ([`super::alcance`]).
    ParAchado(std::net::SocketAddr),
    /// Uma verificação da economia de energia do Wi-Fi ([`super::energia`]).
    Economia(ir_energia::Economia),
    /// O rádio abriu depois da subida — o canal estava ocupado ([`ir_transporte::abrir`]).
    Radio(ir_transporte::RadioAberto),
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
    /// Quantos ajudantes de clipboard estão ligados, para o diagnóstico.
    pub(crate) ajudantes: crate::ipc::Ajudantes,
    /// Quem está por perto para parear: rede e rádio.
    pub(crate) descoberta: ir_transporte::Descoberta,
    /// Por onde pedir um envio de arquivos.
    ///
    /// Só isto: o ator não conduz transferência, não conhece o socket de dados e não vê bloco
    /// nenhum. Ele encaminha o pedido e segue no compasso da entrada.
    pub(crate) arquivos: ir_transferencia::Pedidos,
    /// O endereço do rádio desta máquina, quando há rádio e ele diz.
    pub(crate) radio_proprio: Option<ir_proto::ids::RadioAddress>,
    /// Por onde as tarefas de fundo devolvem o resultado ao ator.
    pub(crate) de_fundo: tokio::sync::mpsc::UnboundedSender<DeFundo>,
}

impl Daemon {
    /// Monta o ator.
    #[must_use]
    pub(crate) fn new(parts: Parts) -> Self {
        let alcance = super::alcance::da_configuracao(&parts.config);
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
            discagem: None,
            // Ninguém fixou nada até a interface pedir: a escolha começa automática.
            portador_fixado: None,
            seed_pointer: true,
            alcance,
            radio_proprio: parts.radio_proprio,
            economia_aqui: ir_energia::Economia::Desconhecida,
            economia_no_par: None,
            de_fundo: parts.de_fundo,
            ticks: 0,
            avisos: parts.avisos,
            machine: parts.machine,
            nome: parts.nome,
            edge: parts.edge,
            last_phase: Phase::Offline,
            agente: parts.agente,
            agente_pronto: false,
            identidade_local: parts.identidade_local,
            arquivos: parts.arquivos,
            descoberta: parts.descoberta,
            ajudantes: parts.ajudantes,
            ultimo_arranjo: None,
        }
    }
}
