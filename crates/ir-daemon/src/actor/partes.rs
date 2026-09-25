//! Como o ator nasce, e por onde ele é alimentado.
//!
//! Duas coisas do mesmo assunto: [`Parts`], o que o [`Daemon`] precisa para existir, e
//! [`Entradas`], as origens de evento que o mantêm vivo. Ficam fora do laço central por tamanho
//! — o laço é o que se lê para entender o serviço, e uma lista de campos no meio dele só atrapalha.

use std::sync::Arc;
use std::time::Instant;

use ir_input::{Capturer, Injector};
use ir_ipc::{Aviso, ComandoDoAgente, FatoDoAgente, Maquina, Nome};
use ir_session::{CommandBatch, Input, LocalIdentity, Phase, Session};
use tokio::sync::broadcast;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};

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
    /// O valor da política de Ctrl+Alt+Del a devolver mudou, para a configuração guardá-lo
    /// (Windows, [`super::protegido`]).
    #[cfg_attr(not(windows), allow(dead_code))]
    PoliticaDeAtencao(Option<u32>),
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
    /// Diretório de estado.
    pub(crate) data_dir: std::path::PathBuf,
    /// A configuração corrente.
    pub(crate) config: Config,
    /// Emissor de avisos para as interfaces.
    pub(crate) avisos: broadcast::Sender<Aviso>,
    /// Esta máquina, já no vocabulário da interface.
    pub(crate) machine: Maquina,
    /// A impressão digital desta máquina, a mesma do registro.
    pub(crate) impressao: String,
    /// O nome desta máquina.
    pub(crate) nome: Nome,
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
    pub(crate) de_fundo: UnboundedSender<DeFundo>,
}

impl Daemon {
    /// Monta o ator.
    #[must_use]
    pub(crate) fn new(parts: Parts) -> Self {
        let mut daemon = Self {
            alcance: super::alcance::da_configuracao(&parts.config),
            // `ip:porta` ou endereço de rádio: é o endereço que diz o portador.
            peer: parts.config.endereco_do_par().and_then(Endereco::ler),
            #[cfg(windows)]
            atencao: super::protegido::aplicador_de_atencao(&parts.config, &parts.de_fundo),
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
            gravador: crate::config::Gravador::novo(parts.data_dir.clone()),
            data_dir: parts.data_dir,
            config: parts.config,
            pareamento: None,
            discagem: None,
            seed_pointer: true,
            radio_proprio: parts.radio_proprio,
            economia_aqui: ir_energia::Economia::Desconhecida,
            economia_no_par: None,
            de_fundo: parts.de_fundo,
            ticks: 0,
            avisos: parts.avisos,
            machine: parts.machine,
            impressao: parts.impressao,
            nome: parts.nome,
            last_phase: Phase::Offline,
            agente: parts.agente,
            agente_pronto: false,
            zelador_do_agente: super::agente::zelador(),
            suprimindo: false,
            dormindo: false,
            economia_pedida_em: None,
            pareamento_aberto_ate: None,
            abertura_anunciada: None,
            voltas: ir_painel::Voltas::default(),
            ultima_queda: None,
            pausa: None,
            desktops_do_agente: super::agente::DesktopsDoAgente::default(),
            recusa_protegido: false,
            injecao_recusada: None,
            cursor: super::cursor::Conducao::default(),
            par_recusa_protegido: false,
            tela_protegida: false,
            borda_travada: false,
            identidade_local: parts.identidade_local,
            arquivos: parts.arquivos,
            descoberta: parts.descoberta,
            ajudantes: parts.ajudantes,
            ultimo_arranjo: None,
        };
        daemon.alimentar_sessao_nova();
        daemon
    }

    /// O que toda sessão nova precisa saber ao nascer: o portador fixado, as telas, a borda travada,
    /// o rádio daqui e a economia do Wi-Fi.
    ///
    /// Um ponto só para a subida e para a sessão recriada numa mudança de política. Eram dois, e a
    /// recriada esquecia a economia do Wi-Fi: até a próxima verificação, 30 s depois, o par não
    /// sabia que a placa daqui cochila.
    pub(crate) fn alimentar_sessao_nova(&mut self) {
        self.session
            .pin_carrier(self.config.fixado(), &mut self.out);
        self.apply_commands();
        if let Some(arranjo) = self.ultimo_arranjo.clone() {
            self.drive(Input::LocalScreens(arranjo));
        }
        if self.borda_travada {
            self.drive(Input::LockEdge(true));
        }
        self.anunciar_radio_proprio();
        if let Some(economia) = self.economia_no_protocolo() {
            self.drive(Input::LocalNetworkPower(economia));
        }
    }

    /// Uma tarefa que bloqueia, numa thread de bloqueio, com o caminho de volta ao ator.
    ///
    /// Sem runtime — os testes síncronos do ator — não roda nada; o resto do serviço segue igual.
    pub(crate) fn em_fundo(&self, tarefa: impl FnOnce(UnboundedSender<DeFundo>) + Send + 'static) {
        if let Ok(runtime) = tokio::runtime::Handle::try_current() {
            let de_fundo = self.de_fundo.clone();
            runtime.spawn_blocking(move || tarefa(de_fundo));
        }
    }

    /// Um futuro, numa tarefa própria, com o caminho de volta ao ator. Sem runtime, nada.
    pub(crate) fn em_fundo_async<F>(&self, tarefa: impl FnOnce(UnboundedSender<DeFundo>) -> F)
    where
        F: Future<Output = ()> + Send + 'static,
    {
        if let Ok(runtime) = tokio::runtime::Handle::try_current() {
            runtime.spawn(tarefa(self.de_fundo.clone()));
        }
    }
}
