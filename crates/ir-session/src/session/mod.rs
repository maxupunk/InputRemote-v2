//! A máquina de estados do produto.
//!
//! Entra um evento, sai uma lista de comandos. Sem E/S, sem relógio lido, sem `async`, sem
//! estado compartilhado — [ADR-0004](../../../docs/adr/0004-nucleo-sans-io.md).

mod agent;
mod area;
mod consultas;
mod direction;
mod edge;
mod frames;
mod incarnation;
mod link;
mod power;
mod reach;
mod receiving;
mod route;
mod secure;
mod sending;
pub mod state;
mod upkeep;

use ir_geometry::{Desktop, Point};
use ir_proto::carrier::Carrier;
use ir_proto::channel::ChannelId;
use ir_proto::frame::{Frame, Sequence};
use ir_proto::ids::RadioAddress;
use ir_proto::input::{InputState, PointerDelta};
use ir_proto::message::Message;

use crate::config::SessionConfig;
use crate::event::{Command, CommandBatch, Input, LinkDown, Notice};
use crate::phase::Phase;
use crate::reliability::{ReliableChannels, SendOutcome};
use crate::sequences::Sequences;
use crate::time::Timestamp;

pub use direction::RECLAIM_DISTANCE;
use ir_confiabilidade::incarnation::Incarnations;
pub use route::{CarrierWins, Route, RouteReport};
pub use state::{CarrierSet, Clock, LocalIdentity, PeerInfo};

/// A sessão.
///
/// Toda a lógica do produto está aqui e nos módulos irmãos. Não há relógio, socket, arquivo
/// nem chamada de sistema em nenhum deles — o que permite testar travessia de borda,
/// reconexão, troca de portador e liberação de teclas com `cargo test`, em milissegundos,
/// sem rádio e sem segundo computador.
#[derive(Debug, Clone)]
pub struct Session {
    pub(super) config: SessionConfig,
    pub(super) identity: LocalIdentity,
    pub(super) phase: Phase,

    /// Por quais portadores de entrada a sessão fala, quando há sessão ([`route`]).
    pub(super) route: Option<Route>,
    pub(super) available: CarrierSet,
    /// Portador fixado pelo usuário, se houver.
    pub(super) pinned: Option<Carrier>,

    pub(super) peer: Option<PeerInfo>,
    pub(super) local_screens: Option<Desktop>,
    pub(super) peer_screens: Option<Desktop>,

    /// Onde o ponteiro está **nesta** máquina.
    ///
    /// Com o controle aqui, é o ponteiro local; com o par usando esta tela, é onde ele está —
    /// é assim que a travessia de volta é detectada do lado certo.
    pub(super) pointer: Point,

    /// O que está pressionado.
    ///
    /// Mandando, é o que foi enviado; recebendo, é o que foi injetado. É a mesma estrutura nos
    /// dois sentidos justamente para que a reconciliação seja uma comparação.
    pub(super) input_state: InputState,

    /// Se o agente local está pronto para injetar.
    pub(super) agent_ready: bool,

    pub(super) seqs: Sequences,

    /// Janelas de retransmissão e detecção de repetição, por canal.
    ///
    /// Só têm efeito sobre portador de datagrama; sobre stream o portador já garante ordem e
    /// entrega, e as janelas ficam vazias.
    pub(super) reliability: ReliableChannels,

    /// A encarnação desta sessão e a do par, para descartar quadros de sessões que acabaram.
    pub(super) incarnations: Incarnations,
    pub(super) clock: Clock,

    /// A última ida e volta medida até o par.
    ///
    /// Parte do estado observável exigido por `docs/01-visao-e-escopo.md` §5: a interface
    /// mostra o portador ativo **e** a latência dele.
    pub(super) last_rtt: Option<crate::time::Millis>,
    /// A tecla que disparou um atalho, para a subida dela também não ir ao par ([`secure`]).
    pub(super) swallowed: Option<ir_proto::input::HidUsage>,
    /// Os modificadores apertados neste teclado, encaminhados ou não — para os atalhos.
    pub(super) held_here: ir_proto::input::Modifiers,
    /// Se a borda está travada: o ponteiro não atravessa ([`secure`]).
    pub(super) edge_locked: bool,

    /// Movimento de ponteiro ainda não despachado.
    ///
    /// Coalescido: se três amostras se acumulam, vai a soma. Perder amostra intermediária é
    /// invisível; atrasar não é (`docs/02-arquitetura.md` §6, regra 5).
    pub(super) pending_pointer: PointerDelta,

    /// O canal 4: o texto do clipboard indo e vindo.
    pub(super) area: ir_area::Area,

    /// O endereço do rádio desta máquina, para contar ao par ([`reach`]).
    pub(super) local_radio: Option<RadioAddress>,

    /// A sequência da última amostra de ponteiro aplicada.
    ///
    /// O canal 2 não tem confirmação, mas tem ordem: amostra com sequência igual ou anterior é
    /// descartada (`docs/03-protocolo.md` §4.2). Na rota dupla toda amostra chega duas vezes, e o
    /// movimento é relativo — sem este filtro, o cursor andaria o dobro.
    pub(super) last_pointer_rx: Option<Sequence>,

    /// Por qual portador cada quadro novo chegou primeiro — o placar da rota dupla.
    pub(super) wins: CarrierWins,

    /// A economia de energia do Wi-Fi daqui, para contar ao par ([`power`]).
    pub(super) local_power: Option<ir_proto::message::NetworkPowerSaving>,

    /// Enquanto o par usa esta tela, o que o daqui mexeu — para retomar ([`direction`]).
    pub(super) reclaim_watch: direction::ReclaimWatch,
}

impl Session {
    /// Uma sessão nova, desconectada.
    #[must_use]
    pub fn new(config: SessionConfig, identity: LocalIdentity) -> Self {
        Self {
            config,
            identity,
            phase: Phase::Offline,
            route: None,
            available: CarrierSet::NONE,
            pinned: None,
            peer: None,
            local_screens: None,
            peer_screens: None,
            pointer: Point::ORIGIN,
            input_state: InputState::released(),
            agent_ready: false,
            seqs: Sequences::new(),
            reliability: ReliableChannels::new(),
            incarnations: Incarnations::new(config.incarnation_seed),
            clock: Clock::default(),
            pending_pointer: PointerDelta::ZERO,
            last_rtt: None,
            swallowed: None,
            held_here: ir_proto::input::Modifiers::NONE,
            edge_locked: false,
            area: ir_area::Area::default(),
            local_radio: None,
            last_pointer_rx: None,
            wins: CarrierWins::default(),
            local_power: None,
            reclaim_watch: direction::ReclaimWatch::default(),
        }
    }

    /// Fixa um portador, desligando a degradação automática e a rota dupla.
    ///
    /// Quem fixa Bluetooth excluiu a rede de propósito: a partir daí, falhar é falhar, e não
    /// vira outro caminho em silêncio. Com a sessão de pé na rota dupla, a rota estreita para o
    /// portador fixado na hora, sem refazer a sessão; soltar a fixação volta a juntar os dois.
    pub fn pin_carrier(&mut self, carrier: Option<Carrier>, out: &mut CommandBatch) {
        self.pinned = carrier;
        self.apply_pin_to_route(out);
    }

    /// Sincroniza o ponteiro com a posição **absoluta** real da máquina, sem detectar travessia.
    ///
    /// O servidor rastreia a posição acumulando deltas; se o ponto de partida não for o cursor
    /// real, a travessia dispararia na coordenada errada. A periferia chama isto ao estabelecer a
    /// sessão (e ao retomar o controle) para semear a posição verdadeira; os deltas seguintes a
    /// mantêm em sincronia. Não atravessa: é semeadura, não movimento.
    pub fn sync_pointer(&mut self, x: i32, y: i32) {
        let point = Point::new(x, y);
        self.pointer = self
            .local_screens
            .as_ref()
            .map_or(point, |desktop| desktop.nearest_valid(point));
    }

    /// Processa um evento.
    ///
    /// O instante é parâmetro, nunca lido de um relógio. Os comandos vão para `out`, que
    /// quem chama possui e reaproveita entre passos — o caminho quente não aloca.
    ///
    /// `out` **não** é esvaziado aqui: quem chama decide se quer acumular vários passos antes
    /// de despachar. O uso normal é `out.clear()` antes de cada `step`.
    pub fn step(&mut self, now: Timestamp, input: Input, out: &mut CommandBatch) {
        match input {
            Input::Tick => self.on_tick(now, out),
            Input::CarrierUp(carrier) => self.on_carrier_up(now, carrier, out),
            Input::CarrierDown { carrier, reason } => {
                self.on_carrier_down(now, carrier, reason, out);
            }
            Input::Received { carrier, frame } => self.on_frame(now, carrier, frame, out),
            Input::LocalPointer(delta) => self.on_local_pointer(now, delta, out),
            Input::LocalWheel(delta) => self.on_local_wheel(now, delta, out),
            Input::LocalKey { usage, pressed } => self.on_local_key(now, usage, pressed, out),
            Input::LocalButton { button, pressed } => {
                self.on_local_button(now, button, pressed, out);
            }
            Input::EmergencyRelease => self.on_emergency(now, out),
            Input::LocalScreens(layout) => self.on_local_screens(now, layout, out),
            Input::SetPeerEdge { edge, chosen_at } => {
                self.on_set_peer_edge(now, edge, chosen_at, out);
            }
            Input::AgentReady => self.agent_ready = true,
            Input::AgentLost => self.on_agent_lost(now, out),
            Input::ClipboardText(texto) => self.on_clipboard_text(now, texto, out),
            Input::LocalRadio(radio) => self.on_local_radio(now, radio, out),
            Input::LocalNetworkPower(state) => self.on_local_network_power(now, state, out),
            Input::SecureAttention => self.request_secure_attention(now, out),
            Input::LocalProtectedDesktop(refused) => {
                self.on_local_protected_desktop(now, refused, out);
            }
            Input::LockEdge(locked) => self.edge_locked = locked,
            Input::LockPeerScreen => self.request_peer_lock(now, out),
            Input::DisablePeerNetworkPowerSaving => {
                if !self.on_disable_peer_network_power(now, out) {
                    out.push(Command::Notify(Notice::PeerCannotFixNetworkPower));
                }
            }
        }
    }

    /// Monta e despacha um quadro por todos os portadores da rota.
    ///
    /// Sem rota, nada acontece: é o caso normal entre a queda e a reconexão, e enfileirar
    /// silenciosamente seria pior — a mensagem chegaria fora de contexto.
    pub(super) fn send(&mut self, now: Timestamp, message: Message, out: &mut CommandBatch) {
        let Some(route) = self.route else { return };
        let channel = message.channel();
        if !route.carriers().all(|carrier| channel.allows(carrier)) {
            // Não deveria acontecer: quem monta a mensagem escolhe o canal. Descartar em
            // silêncio esconderia o defeito, então isto vira aviso para a interface.
            out.push(Command::Notify(Notice::ProtocolError {
                code: ir_proto::message::ErrorCode::ChannelViolation,
                fatal: false,
            }));
            return;
        }

        let seq = self.seqs.next(channel);
        let mut frame = Frame::new(message, seq).in_epoch(self.incarnations.local());

        // Pega uma confirmação para carregar de volta. Aproveitar um quadro que já vai sair é
        // de graça, e é o que evita mandar `AckOnly` na maioria dos casos.
        //
        // Todo canal confiável tem confiabilidade de aplicação, qualquer que seja o portador: a
        // sessão trata toda rota como datagrama ([`route`]).
        let reliable = ReliableChannels::covers(channel);
        if reliable && let Some(pending) = self.ack_to_piggyback() {
            frame = frame.with_ack(pending.channel, pending.ack);
        }

        // Uma confirmação pura **não** entra na janela e nunca é retransmitida. Se entrasse,
        // cada confirmação precisaria ser confirmada, e o laço encheria a janela do canal de
        // controle até derrubar a sessão. É a mesma razão pela qual um ACK puro de TCP não
        // carrega sequência a confirmar.
        if reliable
            && !is_bare_ack(&frame)
            && self.reliability.on_sent(channel, now, seq, &frame) == SendOutcome::WindowFull
        {
            // Janela cheia é o único caso em que não se pode nem enviar nem descartar: o
            // descarte perderia um evento de teclado em silêncio. A política de saturação de
            // canal confiável é derrubar o enlace, e é o que se faz.
            self.tear_down(now, LinkDown::TransportFailed, out);
            return;
        }

        self.dispatch_on_route(frame, out);
    }

    /// A confirmação mais urgente a carregar num quadro que já vai sair.
    ///
    /// A ordem é a de importância: entrada antes de controle, porque é a janela da entrada que
    /// enche durante digitação contínua e é ela que derrubaria a sessão no meio de uma frase.
    fn ack_to_piggyback(&self) -> Option<ir_proto::frame::ChannelAck> {
        const ORDER: [ChannelId; 4] = [
            ChannelId::ReliableInput,
            ChannelId::Control,
            ChannelId::Feedback,
            ChannelId::ClipboardText,
        ];
        ORDER.into_iter().find_map(|channel| {
            self.reliability
                .ack_for(channel)
                .map(|ack| ir_proto::frame::ChannelAck::new(channel, ack))
        })
    }
}

/// Se este quadro é uma confirmação pura.
///
/// Reconhecido pelo conteúdo e não por uma marca separada: `AckOnly` existe justamente para
/// carregar nada além da confirmação, e duas formas de dizer a mesma coisa divergiriam.
pub(super) fn is_bare_ack(frame: &Frame) -> bool {
    matches!(
        frame.message,
        Message::Control(ir_proto::message::Control::AckOnly)
    )
}

/// Se este quadro é um adeus.
///
/// Como a confirmação pura, ele viaja fora do fluxo ordenado: é a última coisa que se manda,
/// não há quem o confirme, e esperar a ordem dele seria esperar por uma sessão que já acabou.
pub(super) fn is_farewell(frame: &Frame) -> bool {
    matches!(
        frame.message,
        Message::Control(ir_proto::message::Control::Bye { .. })
    )
}

/// Erro ao construir uma sessão com configuração incoerente.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ConfigError {
    /// Os prazos não fazem sentido entre si.
    #[error(
        "prazos incoerentes: o heartbeat, o snapshot e as retransmissões precisam caber no prazo de queda"
    )]
    IncoherentTimings,
}

impl Session {
    /// Uma sessão nova, recusando configuração incoerente.
    ///
    /// # Errors
    ///
    /// [`ConfigError::IncoherentTimings`] quando os prazos se contradizem — por exemplo um
    /// *heartbeat* mais lento que o prazo de queda, que faria a sessão cair sozinha a cada
    /// ciclo. Ver [`Timings::is_coherent`](crate::config::Timings::is_coherent).
    pub fn try_new(config: SessionConfig, identity: LocalIdentity) -> Result<Self, ConfigError> {
        if config.timings.is_coherent() {
            Ok(Self::new(config, identity))
        } else {
            Err(ConfigError::IncoherentTimings)
        }
    }

    /// Muda de fase, recusando transição que não existe.
    ///
    /// Uma transição inválida é defeito de programação, não condição de execução: em vez de
    /// entrar em pânico num processo `SYSTEM`, a sessão fica onde está e avisa. Ficar parado
    /// é sempre seguro; o `ReleaseAll` de qualquer falha continua valendo.
    pub(super) fn move_to(&mut self, next: Phase, out: &mut CommandBatch) -> bool {
        if self.phase == next {
            return true;
        }
        if !self.phase.can_move_to(next) {
            out.push(Command::Notify(Notice::ProtocolError {
                code: ir_proto::message::ErrorCode::StateDesync,
                fatal: false,
            }));
            return false;
        }
        self.phase = next;
        true
    }

    /// Deriva o motivo de queda a partir do que o par mandou.
    pub(super) const fn peer_closed(reason: ir_proto::message::DisconnectReason) -> LinkDown {
        LinkDown::PeerClosed(reason)
    }
}
