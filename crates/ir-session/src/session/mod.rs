//! A máquina de estados do produto.
//!
//! Entra um evento, sai uma lista de comandos. Sem E/S, sem relógio lido, sem `async`, sem
//! estado compartilhado — [ADR-0004](../../../docs/adr/0004-nucleo-sans-io.md).

mod client;
mod consultas;
mod edge;
mod frames;
mod incarnation;
mod link;
mod server;
pub mod state;

use ir_geometry::{Desktop, Point};
use ir_proto::carrier::Carrier;
use ir_proto::channel::ChannelId;
use ir_proto::frame::Frame;
use ir_proto::input::{InputState, PointerDelta};
use ir_proto::message::Message;
use ir_proto::screens::ScreenLayout;

use crate::config::{Role, SessionConfig};
use crate::event::{Command, CommandBatch, Input, LinkDown, Notice};
use crate::phase::Phase;
use crate::reliability::{ReliableChannels, SendOutcome};
use crate::sequences::Sequences;
use crate::time::Timestamp;

use incarnation::Incarnations;
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

    /// O portador de entrada em uso, quando há sessão.
    pub(super) carrier: Option<Carrier>,
    pub(super) available: CarrierSet,
    /// Portador fixado pelo usuário, se houver.
    pub(super) pinned: Option<Carrier>,

    pub(super) peer: Option<PeerInfo>,
    pub(super) local_screens: Option<Desktop>,
    pub(super) peer_screens: Option<Desktop>,

    /// Onde o ponteiro está **nesta** máquina.
    ///
    /// No servidor, enquanto o controle é local. No cliente, enquanto ele está sendo
    /// controlado — é assim que a travessia de volta é detectada do lado certo.
    pub(super) pointer: Point,

    /// O que está pressionado.
    ///
    /// No servidor é o que foi enviado; no cliente é o que foi injetado. É a mesma estrutura
    /// nos dois lados justamente para que a reconciliação seja uma comparação.
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

    /// Movimento de ponteiro ainda não despachado.
    ///
    /// Coalescido: se três amostras se acumulam, vai a soma. Perder amostra intermediária é
    /// invisível; atrasar não é (`docs/02-arquitetura.md` §6, regra 5).
    pub(super) pending_pointer: PointerDelta,
}

impl Session {
    /// Uma sessão nova, desconectada.
    #[must_use]
    pub fn new(config: SessionConfig, identity: LocalIdentity) -> Self {
        Self {
            config,
            identity,
            phase: Phase::Offline,
            carrier: None,
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
        }
    }

    /// Fixa um portador, desligando a degradação automática.
    ///
    /// Quem fixa Bluetooth excluiu a rede de propósito: a partir daí, falhar é falhar, e não
    /// vira outro caminho em silêncio.
    pub const fn pin_carrier(&mut self, carrier: Option<Carrier>) {
        self.pinned = carrier;
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
            Input::SetPeerEdge(edge) => self.on_set_peer_edge(now, edge, out),
            Input::AgentReady => self.agent_ready = true,
            Input::AgentLost => self.on_agent_lost(now, out),
        }
    }

    /// Atualiza o arranjo local e conta ao par.
    fn on_local_screens(&mut self, now: Timestamp, layout: ScreenLayout, out: &mut CommandBatch) {
        self.local_screens = Desktop::from_layout(&layout);
        if let Some(desktop) = self.local_screens.as_ref() {
            // A posição guardada pode ter ficado fora de qualquer tela quando um monitor foi
            // removido. Trazer de volta aqui evita coordenada inválida em todo o resto.
            self.pointer = desktop.nearest_valid(self.pointer);
        }
        if self.phase.is_established() {
            self.send(
                now,
                Message::Control(ir_proto::message::Control::Screens(layout)),
                out,
            );
        }
    }

    /// O agente local sumiu.
    ///
    /// Se havia entrada em curso, solta tudo antes de qualquer outra coisa: o agente novo vai
    /// nascer sem saber o que estava pressionado, e o estado é do serviço exatamente para
    /// isto (`docs/02-arquitetura.md` §1.1).
    fn on_agent_lost(&mut self, now: Timestamp, out: &mut CommandBatch) {
        self.agent_ready = false;
        if self.phase.may_hold_input() {
            self.release_everything(out);
            if self.config.role == Role::Client {
                // Devolve o controle: sem agente não há como injetar, e segurar o ponteiro
                // do usuário do outro lado seria pior.
                self.report_edge_return(now, out);
            }
        }
    }

    /// Solta tudo, local e logicamente.
    pub(super) fn release_everything(&mut self, out: &mut CommandBatch) {
        self.input_state.release_all();
        out.push(Command::ReleaseAll);
    }

    /// Monta e despacha um quadro pelo portador ativo.
    ///
    /// Sem portador ativo, nada acontece: é o caso normal entre a queda e a reconexão, e
    /// enfileirar silenciosamente seria pior — a mensagem chegaria fora de contexto.
    pub(super) fn send(&mut self, now: Timestamp, message: Message, out: &mut CommandBatch) {
        let Some(carrier) = self.carrier else { return };
        let channel = message.channel();
        if !channel.allows(carrier) {
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
        if channel.needs_app_reliability(carrier)
            && let Some(pending) = self.ack_to_piggyback()
        {
            frame = frame.with_ack(pending.channel, pending.ack);
        }

        // Uma confirmação pura **não** entra na janela e nunca é retransmitida. Se entrasse,
        // cada confirmação precisaria ser confirmada, e o laço encheria a janela do canal de
        // controle até derrubar a sessão. É a mesma razão pela qual um ACK puro de TCP não
        // carrega sequência a confirmar.
        if channel.needs_app_reliability(carrier)
            && !is_bare_ack(&frame)
            && self.reliability.on_sent(channel, now, seq, &frame) == SendOutcome::WindowFull
        {
            // Janela cheia é o único caso em que não se pode nem enviar nem descartar: o
            // descarte perderia um evento de teclado em silêncio. A política de saturação de
            // canal confiável é derrubar o enlace, e é o que se faz.
            self.tear_down(now, LinkDown::TransportFailed, out);
            return;
        }

        out.push(Command::Send { carrier, frame });
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
