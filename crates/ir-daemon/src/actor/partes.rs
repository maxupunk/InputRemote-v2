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
    /// O sistema vai dormir, acordou, ou trocou de sessão ([`super::sistema`]).
    Sistema(super::EventoDoSistema),
    /// A tela desta máquina está, ou deixou de estar, bloqueada ou no login (Linux).
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    TelaProtegida(bool),
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
    /// Quem abre o rádio de novo quando ele é perdido. `None` nos testes.
    pub(crate) reabridor: Option<ir_transporte::Reabridor>,
    /// Injetor (cliente) ou nada.
    pub(crate) injector: Option<Box<dyn Injector>>,
    /// Capturador (servidor) ou nada.
    pub(crate) capturer: Option<Box<dyn Capturer>>,
    /// Por onde a captura chega ao ator — para começá-la se a máquina virar servidor depois.
    pub(crate) captura: tokio::sync::mpsc::UnboundedSender<ir_input::CaptureEvent>,
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
        let fixado = ir_painel::portador_do_texto(parts.config.portador_fixado.as_deref());
        let mut daemon = Self {
            session: parts.session,
            out: CommandBatch::with_capacity(32),
            start: Instant::now(),
            rede: parts.rede,
            radio: parts.radio,
            reabridor: parts.reabridor,
            injector: parts.injector,
            capturer: parts.capturer,
            captura: parts.captura,
            screen: parts.screen,
            peer: parts.peer,
            gravador: super::gravador::Gravador::novo(parts.data_dir.clone()),
            data_dir: parts.data_dir,
            config: parts.config,
            pareamento: None,
            discagem: None,
            // O que ficou gravado da última vez; sem nada gravado, a escolha é automática.
            portador_fixado: fixado,
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
            suprimindo: false,
            dormindo: false,
            economia_pedida_em: None,
            pareamento_aberto_ate: None,
            abertura_anunciada: None,
            voltas: ir_painel::Voltas::default(),
            ultima_queda: None,
            pausa: None,
            desktops_do_agente: Vec::new(),
            recusa_protegido: false,
            par_recusa_protegido: false,
            tela_protegida: false,
            borda_travada: false,
            identidade_local: parts.identidade_local,
            arquivos: parts.arquivos,
            descoberta: parts.descoberta,
            ajudantes: parts.ajudantes,
            ultimo_arranjo: None,
        };
        daemon.fixar_na_sessao_nova();
        daemon
    }

    /// A sessão nasce já sabendo do portador fixado; os comandos disto, sem sessão de pé, não
    /// têm destino.
    fn fixar_na_sessao_nova(&mut self) {
        if let Some(portador) = self.portador_fixado {
            self.session
                .pin_carrier(Some(portador.no_protocolo()), &mut self.out);
            self.out.clear();
        }
    }
}
