//! O ator central: o único dono do [`Session`], reagindo a rede, entrada e relógio.
//!
//! É a tarefa da sessão de [02, §4](../../../docs/02-arquitetura.md): tudo que muda o estado
//! passa por aqui, um evento de cada vez. Rede e entrada só convertem bytes em [`Input`] e
//! comandos em bytes ([`commands`](crate::commands)).

use std::net::SocketAddr;
use std::time::Instant;

use ir_input::{CaptureEvent, Capturer, Injector};
use ir_ipc::{Aviso, Maquina, Nome};
use ir_net::{ConnectMode, NetCommand, NetEvent};
use ir_proto::carrier::Carrier;
use ir_proto::input::PointerDelta;
use ir_proto::screens::Edge;
use ir_session::{CommandBatch, Input, LinkDown, Phase, Session, Timestamp};
use tokio::sync::broadcast;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};
use tracing::{error, info, warn};

use crate::config::{Config, PinnedPeer, encode_key};
use crate::ipc::PedidoRecebido;

mod pedidos;

/// A entrada de captura, já convertida para o canal do ator.
pub(crate) type CaptureRx = UnboundedReceiver<CaptureEvent>;

/// O ator do serviço.
pub(crate) struct Daemon {
    pub(crate) session: Session,
    pub(crate) out: CommandBatch,
    start: Instant,
    pub(crate) net: UnboundedSender<NetCommand>,
    pub(crate) injector: Option<Box<dyn Injector>>,
    pub(crate) capturer: Option<Box<dyn Capturer>>,
    pub(crate) screen: (u32, u32),
    peer_addr: Option<SocketAddr>,
    data_dir: std::path::PathBuf,
    config: Config,
    pending_peer: Option<ir_crypto::PublicKey>,
    awaiting_confirm: bool,
    /// Se a próxima posição absoluta deve **semear** o ponteiro (sem atravessar) em vez de virar
    /// movimento. Ligado ao estabelecer e ao retomar o controle, para o cursor real e o modelo da
    /// sessão começarem no mesmo ponto.
    seed_pointer: bool,
    /// Se o enlace seguro (criptografia) está de pé. Distinto de a sessão estar estabelecida.
    linked: bool,
    /// Contador de batidas, para espaçar as tentativas de reconexão.
    ticks: u32,
    /// Por onde o serviço empurra avisos para as interfaces conectadas.
    avisos: broadcast::Sender<Aviso>,
    /// Esta máquina, para a impressão digital aparecer na tela de pareamento.
    machine: Maquina,
    /// O nome desta máquina.
    nome: Nome,
    /// A borda que dá para o par.
    edge: Edge,
    /// A última fase informada às interfaces, para só avisar quando muda de verdade.
    last_phase: Phase,
}

/// A cada quantas batidas de 5 ms se tenta reconectar. 600 × 5 ms = 3 s.
const RECONNECT_TICKS: u32 = 600;

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
            awaiting_confirm: false,
            seed_pointer: true,
            linked: false,
            ticks: 0,
            avisos: parts.avisos,
            machine: parts.machine,
            nome: parts.nome,
            edge: parts.edge,
            last_phase: Phase::Offline,
        }
    }

    /// O instante corrente, do relógio monotônico. Nunca lido dentro da sessão.
    fn now(&self) -> Timestamp {
        let micros = u64::try_from(self.start.elapsed().as_micros()).unwrap_or(u64::MAX);
        Timestamp::from_micros(micros)
    }

    /// Alimenta um evento à sessão e executa os comandos que ela produzir.
    fn drive(&mut self, input: Input) {
        let now = self.now();
        self.session.step(now, input, &mut self.out);
        self.apply_commands();
    }

    /// A batida periódica: reconecta quando é hora, depois avança a sessão.
    fn on_tick(&mut self) {
        self.ticks = self.ticks.wrapping_add(1);
        // A cada ~3 s, tenta se recuperar do que estiver caído, para a ordem de subida das duas
        // máquinas não importar e uma falha transitória não exigir reiniciar à mão.
        if self.ticks.is_multiple_of(RECONNECT_TICKS) {
            self.reconnect_if_needed();
        }
        self.drive(Input::Tick);
        self.notar_estado();
    }

    /// Avisa as interfaces se a fase da sessão mudou desde o último aviso.
    fn notar_estado(&mut self) {
        let fase = self.session.phase();
        if fase != self.last_phase {
            self.last_phase = fase;
            let _ = self.avisos.send(Aviso::EstadoMudou(self.estado()));
        }
    }

    /// Retoma a conexão conforme o que está caído.
    fn reconnect_if_needed(&mut self) {
        if self.awaiting_confirm {
            return; // no meio de um pareamento; não atrapalhar
        }
        if self.linked {
            // O enlace seguro está de pé, mas a sessão caiu (silêncio do par). Reinicia a sessão
            // sobre o mesmo enlace: um `Hello` novo, que o par absorve se já estiver de pé.
            if self.session.phase() == Phase::Offline {
                self.drive(Input::CarrierUp(Carrier::Udp));
            }
        } else {
            // Sem enlace: se somos o iniciador (temos endereço), tenta conectar de novo.
            self.connect_if_possible();
        }
    }

    /// Roda o ator até os canais fecharem.
    pub(crate) async fn run(
        mut self,
        mut net_events: UnboundedReceiver<NetEvent>,
        mut capture: CaptureRx,
        mut confirm: UnboundedReceiver<String>,
        mut pedidos: UnboundedReceiver<PedidoRecebido>,
    ) {
        // Bate a sessão a cada 5 ms: é o que faz os prazos (heartbeat, snapshot, retransmissão,
        // queda por tempo) vencerem, sem gerenciar temporizadores um a um.
        let mut ticker = tokio::time::interval(std::time::Duration::from_millis(5));
        loop {
            tokio::select! {
                _ = ticker.tick() => self.on_tick(),
                event = net_events.recv() => match event {
                    Some(event) => self.on_net(event),
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
            }
        }
    }

    /// Um evento vindo da rede.
    fn on_net(&mut self, event: NetEvent) {
        match event {
            NetEvent::PairingCode {
                code, peer_static, ..
            } => self.on_pairing_code(code, peer_static),
            NetEvent::Established { peer_static, peer } => self.on_established(peer_static, peer),
            NetEvent::Frame(bytes) => self.on_frame(&bytes),
            NetEvent::LinkDown(reason) => {
                info!(reason, "enlace de rede caiu");
                self.linked = false;
                self.drive(Input::CarrierDown {
                    carrier: Carrier::Udp,
                    reason: LinkDown::TransportFailed,
                });
                self.notar_estado();
            }
            NetEvent::Error(message) => warn!(message, "erro de rede"),
            _ => {}
        }
    }

    fn on_pairing_code(&mut self, code: [u8; 6], peer_static: ir_crypto::PublicKey) {
        self.pending_peer = Some(peer_static);
        self.awaiting_confirm = true;
        let digits: String = code.iter().map(|d| char::from(b'0' + d)).collect();
        info!("código de pareamento: {digits}");
        println!("\n=== CÓDIGO DE PAREAMENTO: {digits} ===");
        println!("Confere com o outro computador? [s/n] e Enter:");
        // A interface mostra os seis dígitos em caixas para a comparação em voz alta; vão
        // separados, não como texto, exatamente por isso.
        let _ = self
            .avisos
            .send(Aviso::CodigoDePareamento { digitos: code });
    }

    fn on_confirm(&mut self, line: &str) {
        let trimmed = line.trim();
        if !self.awaiting_confirm || trimmed.is_empty() {
            // Uma linha vazia (Enter solto) não é resposta: ignorar, não recusar.
            return;
        }
        let yes = matches!(trimmed.to_lowercase().as_str(), "s" | "sim" | "y" | "yes");
        self.confirmar(yes);
    }

    /// A resposta à comparação do código, venha do terminal ou da interface.
    fn confirmar(&mut self, yes: bool) {
        if !self.awaiting_confirm {
            return;
        }
        self.awaiting_confirm = false;
        info!("confirmação recebida: {}", if yes { "sim" } else { "não" });
        let _ = self.net.send(NetCommand::ConfirmPairing(yes));
        if !yes {
            // Códigos diferentes ou recusa: não há par, e a interface precisa saber que o
            // pareamento terminou sem sucesso para sair da tela de comparação.
            self.pending_peer = None;
            let _ = self
                .avisos
                .send(Aviso::PareamentoConcluido { sucesso: false });
        }
    }

    fn on_established(&mut self, peer_static: ir_crypto::PublicKey, peer: SocketAddr) {
        if self.pending_peer.take().is_some() {
            self.save_peer(peer_static, peer);
            // O par foi gravado: a interface fecha a tela de comparação com sucesso.
            let _ = self
                .avisos
                .send(Aviso::PareamentoConcluido { sucesso: true });
        } else if let Some(pinned) = self.config.first_peer_key()
            && pinned != peer_static
        {
            warn!("a chave do par não confere com a fixada — recusando");
            let _ = self.net.send(NetCommand::Disconnect);
            return;
        }
        self.linked = true;
        self.peer_addr = Some(peer);
        // A próxima posição absoluta semeia o ponteiro: o cursor real está onde está, e o modelo
        // da sessão precisa começar no mesmo ponto, senão a primeira travessia dispara errado.
        self.seed_pointer = true;
        info!(%peer, "enlace seguro pronto; iniciando a sessão");
        self.drive(Input::CarrierUp(Carrier::Udp));
        self.notar_estado();
    }

    fn save_peer(&mut self, peer_static: ir_crypto::PublicKey, peer: SocketAddr) {
        let pinned = PinnedPeer {
            pubkey: encode_key(&peer_static),
            addr: Some(peer.to_string()),
        };
        self.config.peers = vec![pinned];
        if let Err(error) = self.config.save(&self.data_dir) {
            error!(%error, "não foi possível gravar o par");
        } else {
            info!("par gravado");
        }
    }

    fn on_frame(&mut self, bytes: &[u8]) {
        match ir_proto::codec::decode(bytes, Carrier::Udp) {
            Ok(frame) => self.drive(Input::Received {
                carrier: Carrier::Udp,
                frame,
            }),
            Err(error) => warn!(%error, "quadro recebido malformado"),
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

    /// Inicia a conexão como iniciador, se houver par e endereço.
    pub(crate) fn connect_if_possible(&self) {
        let Some(peer) = self.peer_addr else {
            info!("sem endereço de par; aguardando conexão de entrada");
            return;
        };
        let mode = self
            .config
            .first_peer_key()
            .map_or(ConnectMode::Pair, ConnectMode::Reconnect);
        info!(%peer, "conectando ao par");
        let _ = self.net.send(NetCommand::Connect { peer, mode });
    }
}
