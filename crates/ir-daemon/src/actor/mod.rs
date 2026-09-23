//! O ator central: o único dono do [`Session`], reagindo a transporte, entrada e relógio.
//!
//! É a tarefa da sessão de [02, §4](../../../docs/02-arquitetura.md): tudo que muda o estado
//! passa por aqui, um evento de cada vez. Os transportes e a entrada só convertem bytes em
//! [`Input`] e comandos em bytes ([`commands`](crate::commands)).
//!
//! O que depende de **qual** portador está em uso mora em [`enlace`], ao lado.

use std::sync::Arc;
use std::time::Instant;

use ir_input::{CaptureEvent, Capturer, Injector};
use ir_ipc::{Aviso, ComandoDoAgente, Maquina, Nome, Portador};
use ir_proto::input::PointerDelta;
use ir_proto::screens::{Edge, ScreenLayout};
use ir_session::{CommandBatch, Input, LocalIdentity, Phase, Session, Timestamp};
use tokio::sync::broadcast;
use tokio::sync::mpsc::UnboundedReceiver;
use tracing::info;

use crate::config::Config;
use ir_transporte::{Endereco, Transporte};

mod abertura;
mod agente;
mod alcance;
#[cfg(test)]
mod bancada;
mod discagem;
mod energia;
mod enlace;
mod estado;
mod gravador;
mod papel;
mod parada;
mod pareamento;
mod partes;
mod pausa;
mod pedidos;
mod protegido;
mod sistema;

pub(crate) use papel::{nova_sessao, papel_na_subida};
use pareamento::Pareamento;
pub(crate) use partes::{DeFundo, Entradas, Parts};
pub(crate) use sistema::EventoDoSistema;

/// A entrada de captura, já convertida para o canal do ator.
pub(crate) type CaptureRx = UnboundedReceiver<CaptureEvent>;

/// O ator do serviço.
///
/// Os campos lógicos são estados independentes do mundo lá fora — o agente está de pé, a máquina
/// vai dormir, a supressão vale —, e não um estado só disfarçado de vários.
#[allow(clippy::struct_excessive_bools)]
pub(crate) struct Daemon {
    pub(crate) session: Session,
    pub(crate) out: CommandBatch,
    start: Instant,
    /// O transporte de rede. Sempre existe.
    pub(crate) rede: Arc<dyn Transporte>,
    /// O transporte de rádio, quando há rádio nesta máquina.
    pub(crate) radio: Option<Arc<dyn Transporte>>,
    /// Quem abre o rádio de novo quando ele é perdido.
    pub(crate) reabridor: Option<ir_transporte::Reabridor>,
    pub(crate) injector: Option<Box<dyn Injector>>,
    pub(crate) capturer: Option<Box<dyn Capturer>>,
    /// Por onde a captura chega a este ator, para começá-la numa troca de papel.
    #[cfg_attr(windows, allow(dead_code))]
    pub(crate) captura: tokio::sync::mpsc::UnboundedSender<CaptureEvent>,
    pub(crate) screen: (u32, u32),
    /// Onde o par foi visto pela última vez. O endereço diz por qual portador se fala com ele.
    pub(crate) peer: Option<Endereco>,
    pub(crate) data_dir: std::path::PathBuf,
    pub(crate) config: Config,
    /// Grava a configuração fora do laço, em ordem ([`gravador`]).
    pub(crate) gravador: gravador::Gravador,
    /// O pareamento em andamento, do código na tela até o fim ([`pareamento`]).
    ///
    /// Vai além do clique em "São iguais": até o outro lado responder, a reconexão não disca por
    /// cima e o prazo continua valendo (log 25).
    pub(crate) pareamento: Option<Pareamento>,
    /// O pedido de pareamento que saiu daqui e ainda não teve código ([`discagem`]).
    pub(crate) discagem: Option<discagem::Discagem>,
    /// Se a próxima posição absoluta deve **semear** o ponteiro (sem atravessar) em vez de virar
    /// movimento. Ligado ao estabelecer e ao retomar o controle, para o cursor real e o modelo da
    /// sessão começarem no mesmo ponto.
    pub(crate) seed_pointer: bool,
    /// Onde o par está em cada portador, e por quais há enlace seguro de pé ([`alcance`]).
    pub(crate) alcance: ir_transporte::Alcance,
    /// O endereço do rádio desta máquina, para a sessão contar ao par.
    pub(crate) radio_proprio: Option<ir_proto::ids::RadioAddress>,
    /// A economia de energia do Wi-Fi desta máquina, pela última verificação ([`energia`]).
    pub(crate) economia_aqui: ir_energia::Economia,
    /// A do par, pelo que ele contou.
    pub(crate) economia_no_par: Option<ir_proto::message::NetworkPowerSaving>,
    /// Quando o par pediu pela última vez para desligar a economia daqui ([`energia`]).
    pub(crate) economia_pedida_em: Option<Instant>,
    /// Por onde as tarefas de fundo devolvem o resultado ([`DeFundo`]).
    pub(crate) de_fundo: tokio::sync::mpsc::UnboundedSender<DeFundo>,
    /// Contador de batidas, para espaçar as tentativas de reconexão.
    ticks: u32,
    /// Por onde o serviço empurra avisos para as interfaces conectadas.
    pub(crate) avisos: broadcast::Sender<Aviso>,
    /// Esta máquina, para a impressão digital aparecer na tela de pareamento.
    machine: Maquina,
    /// O nome desta máquina.
    nome: Nome,
    /// A borda que dá para o par.
    edge: Edge,
    /// O portador que o usuário fixou, se ele fixou algum.
    ///
    /// Guardado aqui porque a interface precisa vê-lo de volta, e porque é ele que transforma
    /// "escolhido" em "fixado nas preferências" na frase que aparece na tela.
    pub(crate) portador_fixado: Option<Portador>,
    /// A última fase informada às interfaces, para só avisar quando muda de verdade.
    last_phase: Phase,
    /// Por onde o serviço manda comandos ao agente.
    agente: broadcast::Sender<ComandoDoAgente>,
    /// Se há agente conectado e pronto para capturar e injetar.
    agente_pronto: bool,
    /// Se a sessão pediu a supressão da entrada local, que o agente precisa ver renovada.
    pub(crate) suprimindo: bool,
    /// Se a máquina está indo dormir: aí não se disca, até ela acordar ([`sistema`]).
    pub(crate) dormindo: bool,
    /// Até quando a janela de pareamento aberta deixa um pedido de fora entrar ([`abertura`]).
    pub(crate) pareamento_aberto_ate: Option<Instant>,
    /// A última decisão contada aos transportes sobre pedidos de fora.
    pub(crate) abertura_anunciada: Option<bool>,
    /// As voltas medidas na última janela, para a latência da tela.
    pub(crate) voltas: ir_painel::Voltas,
    /// Por que a última sessão caiu, para a tela.
    pub(crate) ultima_queda: Option<ir_ipc::MotivoDaQueda>,
    /// Se o compartilhamento está pausado, e de que lado ([`pausa`]).
    pub(crate) pausa: Option<ir_ipc::Pausa>,
    /// Os desktops em que o agente consegue injetar — `Winlogon` é a tela de bloqueio.
    pub(crate) desktops_do_agente: Vec<String>,
    /// Se esta máquina está recusando digitação do par no desktop protegido ([`protegido`]).
    pub(crate) recusa_protegido: bool,
    /// Se o par disse que recusa digitação daqui no desktop protegido dele.
    pub(crate) par_recusa_protegido: bool,
    /// Se a tela desta máquina está bloqueada ou no login, pelo `logind` (Linux).
    pub(crate) tela_protegida: bool,
    /// Se a borda está travada, pela janela: a sessão recriada nasce sabendo.
    pub(crate) borda_travada: bool,
    /// Quem esta máquina é, para recriar a sessão numa troca de papel ou de borda.
    identidade_local: LocalIdentity,
    /// O último arranjo de telas conhecido, para a sessão recriada nascer sabendo onde ficam.
    ultimo_arranjo: Option<ScreenLayout>,
    /// Por onde pedir um envio de arquivos. O ator encaminha e segue; não conduz nada.
    pub(crate) arquivos: ir_transferencia::Pedidos,
    /// Quem está por perto para parear. A busca roda fora do ator e responde direto à janela.
    pub(crate) descoberta: ir_transporte::Descoberta,
    /// Quantos ajudantes de clipboard estão ligados.
    pub(crate) ajudantes: crate::ipc::Ajudantes,
}

/// A cada quantas batidas de 5 ms se tenta reconectar. 600 × 5 ms = 3 s.
const RECONNECT_TICKS: u32 = 600;

/// A cada quantas batidas se renova a supressão no agente. 200 × 5 ms = 1 s.
const SUPRESSAO_TICKS: u32 = 200;

/// A cada quantas batidas se verifica a economia de energia do Wi-Fi. 6 000 × 5 ms = 30 s.
const ENERGIA_TICKS: u32 = 6_000;

/// A cada quantas batidas se registra o placar da rota dupla. 12 000 × 5 ms = 1 min.
const PLACAR_TICKS: u32 = 12_000;

impl Daemon {
    /// O instante corrente, do relógio monotônico. Nunca lido dentro da sessão.
    fn now(&self) -> Timestamp {
        let micros = u64::try_from(self.start.elapsed().as_micros()).unwrap_or(u64::MAX);
        Timestamp::from_micros(micros)
    }

    /// Alimenta um evento à sessão e executa os comandos que ela produzir.
    pub(crate) fn drive(&mut self, input: Input) {
        let now = self.now();
        self.session.step(now, input, &mut self.out);
        self.apply_commands();
    }

    /// O transporte por onde se fala com o par.
    ///
    /// Pelo endereço dele, e não pelo portador da sessão: durante o pareamento ainda não há
    /// sessão, e responder à comparação de códigos pelo transporte errado deixaria o outro
    /// computador esperando para sempre.
    pub(crate) fn transporte_do_par(&self) -> Option<&dyn Transporte> {
        // Sem endereço do par, o portador da sessão; sem sessão, a rede.
        let portador = self.peer.map(Endereco::portador).or(self.session.carrier());
        self.transporte(portador.unwrap_or(ir_proto::carrier::Carrier::Udp))
    }

    /// A batida periódica: reconecta quando é hora, depois avança a sessão.
    fn on_tick(&mut self) {
        self.ticks = self.ticks.wrapping_add(1);
        // A cada ~3 s, tenta se recuperar do que estiver caído, para a ordem de subida das duas
        // máquinas não importar e uma falha transitória não exigir reiniciar à mão.
        if self.ticks.is_multiple_of(RECONNECT_TICKS) {
            // Antes de reconectar: um código vencido é o que libera a reconexão de novo.
            self.vencer_pareamento_se_preciso();
            self.vencer_discagem_se_preciso();
            self.reconnect_if_needed();
            self.garantir_agente();
            self.anunciar_abertura();
        }
        self.drive(Input::Tick);
        self.notar_estado();
        if self.ticks.is_multiple_of(SUPRESSAO_TICKS) {
            self.renovar_supressao();
        }
        if self.ticks.is_multiple_of(PLACAR_TICKS) {
            self.registrar_placar();
        }
        // A economia volta a cada reconexão do Wi-Fi, se nada a fixar: verificar de vez em quando.
        if self.ticks.is_multiple_of(ENERGIA_TICKS) {
            self.verificar_economia();
        }
    }

    /// O resultado de uma tarefa de fundo, na vez do ator.
    fn on_de_fundo(&mut self, resultado: DeFundo) {
        match resultado {
            DeFundo::ParAchado(endereco) => self.on_par_achado(endereco),
            DeFundo::Economia(economia) => self.on_economia(economia),
            DeFundo::Radio(aberto) => self.on_radio_tardio(aberto),
            DeFundo::Sistema(evento) => self.on_sistema(evento),
            DeFundo::TelaProtegida(protegida) => self.on_tela_protegida(protegida),
        }
    }

    /// Registra, enquanto a rota for dupla, qual portador chega primeiro — o dado que diz se ela
    /// paga o que custa.
    fn registrar_placar(&self) {
        if self.session.route().is_some_and(ir_session::Route::is_dual) {
            info!(rota = %self.session.route_report(self.now()), "rota dupla");
        }
    }

    /// Avisa as interfaces se a fase da sessão mudou desde o último aviso.
    pub(crate) fn notar_estado(&mut self) {
        let fase = self.session.phase();
        if fase != self.last_phase {
            self.last_phase = fase;
            let _ = self.avisos.send(Aviso::EstadoMudou(self.estado()));
        }
    }

    /// Roda o ator até os canais fecharem.
    pub(crate) async fn run(mut self, entradas: Entradas) {
        let Entradas {
            mut transportes,
            mut capture,
            mut confirm,
            mut pedidos,
            mut fatos,
            mut parada,
            mut de_fundo,
        } = entradas;
        // Bate a sessão a cada 5 ms: é o que faz os prazos (heartbeat, snapshot, retransmissão,
        // queda por tempo) vencerem, sem gerenciar temporizadores um a um.
        let mut ticker = tokio::time::interval(std::time::Duration::from_millis(5));
        loop {
            tokio::select! {
                _ = ticker.tick() => self.on_tick(),
                fato = transportes.recv() => match fato {
                    Some(fato) => self.on_fato_do_transporte(fato),
                    None => break,
                },
                event = capture.recv() => {
                    if let Some(event) = event {
                        self.on_capture(event);
                    }
                }
                line = confirm.recv() => {
                    if let Some(line) = line {
                        self.on_confirm(&line);
                    }
                }
                pedido = pedidos.recv() => {
                    if let Some(pedido) = pedido {
                        self.on_pedido(pedido);
                    }
                }
                fato = fatos.recv() => {
                    if let Some(fato) = fato {
                        self.on_fato(fato);
                    }
                }
                resultado = de_fundo.recv() => {
                    if let Some(resultado) = resultado {
                        self.on_de_fundo(resultado);
                    }
                }
                // Parar, ou quem podia pedir parada foi embora: nos dois casos, sair limpo.
                _ = parada.changed() => {
                    self.encerrar().await;
                    break;
                }
            }
        }
    }

    /// Um evento de entrada capturado localmente (papel de servidor).
    fn on_capture(&mut self, event: CaptureEvent) {
        let input = match event {
            CaptureEvent::PointerAbsolute { x, y } => {
                self.on_absolute_pointer(x, y);
                return;
            }
            CaptureEvent::PointerMotion { dx, dy } => Input::LocalPointer(PointerDelta { dx, dy }),
            CaptureEvent::Wheel(delta) => Input::LocalWheel(delta),
            CaptureEvent::Key { usage, pressed } => Input::LocalKey { usage, pressed },
            CaptureEvent::Button { button, pressed } => Input::LocalButton { button, pressed },
            _ => return,
        };
        self.drive(input);
    }

    /// O cursor real está nesta posição absoluta (controle local).
    ///
    /// Na primeira vez após estabelecer, **semeia** o ponteiro da sessão sem atravessar. Depois,
    /// vira o delta desde a posição que a sessão tem — o que mantém o modelo em sincronia com o
    /// cursor real e faz a travessia disparar no ponto certo.
    fn on_absolute_pointer(&mut self, x: i32, y: i32) {
        if self.seed_pointer {
            self.session.sync_pointer(x, y);
            self.seed_pointer = false;
            return;
        }
        let (px, py) = self.session.pointer_xy();
        let (dx, dy) = (x - px, y - py);
        if dx != 0 || dy != 0 {
            self.drive(Input::LocalPointer(PointerDelta { dx, dy }));
        }
    }

    /// O arranjo de telas desta máquina chegou, ou mudou.
    ///
    /// Guardado, e não só repassado: se a sessão for recriada numa troca de papel ou de borda, a
    /// nova precisa nascer sabendo onde ficam as telas, senão a primeira travessia não acha a borda.
    pub(crate) fn definir_telas(&mut self, arranjo: ScreenLayout) {
        self.ultimo_arranjo = Some(arranjo.clone());
        self.drive(Input::LocalScreens(arranjo));
    }
}
