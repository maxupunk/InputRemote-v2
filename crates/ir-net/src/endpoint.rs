//! O endpoint UDP: uma tarefa que é dona do socket e fala com o serviço por canais.
//!
//! É o "task de rede" de [02, §4](../../../docs/02-arquitetura.md): converte comando em bytes no
//! socket e bytes do socket em evento. Não conhece a máquina de estados da sessão — ele move
//! quadros cifrados e coordena o pareamento.
//!
//! O pareamento precisa da confirmação dos **dois** lados antes de qualquer quadro de sessão
//! ([04, §3.2](../../../docs/04-seguranca.md)): o endpoint só declara [`NetEvent::Established`]
//! quando o usuário local confirmou **e** o par confirmou.

use std::net::SocketAddr;
use std::sync::Arc;

use ir_crypto::{Identity, PublicKey};
use tokio::net::UdpSocket;
use tokio::sync::mpsc;

use crate::error::{NetError, Result};
use crate::handshake::{self, ConnectMode};
use crate::link::SecureLink;
use crate::wire::{self, Kind};

/// Buffer de recepção. Cobre o maior datagrama do produto (1200 B de texto claro + folga).
const BUF: usize = 2048;

/// O que o serviço manda ao endpoint.
#[derive(Debug)]
#[non_exhaustive]
pub enum NetCommand {
    /// Comece a conectar, como iniciador.
    Connect {
        /// O endereço do par.
        peer: SocketAddr,
        /// Parear do zero ou reconectar com chave fixada.
        mode: ConnectMode,
    },
    /// Mande este quadro (bytes já codificados de `ir_proto::Frame`) ao par.
    SendFrame(Vec<u8>),
    /// O usuário respondeu à comparação de códigos.
    ConfirmPairing(bool),
    /// Encerre o enlace atual.
    Disconnect,
    /// Encerre a tarefa.
    Shutdown,
}

/// O que o endpoint conta ao serviço.
#[derive(Debug)]
#[non_exhaustive]
pub enum NetEvent {
    /// O handshake de pareamento terminou; aqui está o código para o usuário comparar.
    PairingCode {
        /// Os seis dígitos.
        code: [u8; 6],
        /// A chave estática que o par apresentou, para gravar após a confirmação.
        peer_static: PublicKey,
        /// O endereço do par.
        peer: SocketAddr,
    },
    /// O enlace está pronto: pareamento confirmado dos dois lados, ou reconexão fixada.
    Established {
        /// A chave estática do par.
        peer_static: PublicKey,
        /// O endereço do par.
        peer: SocketAddr,
    },
    /// Chegou um quadro do par (bytes de `ir_proto::Frame`).
    Frame(Vec<u8>),
    /// O enlace caiu.
    LinkDown(&'static str),
    /// Um erro de rede que não derruba a tarefa.
    Error(String),
}

/// Estado interno do endpoint.
enum State {
    Idle,
    AwaitingConfirm {
        link: SecureLink,
        peer_static: PublicKey,
        local_ok: bool,
        peer_ok: bool,
    },
    Established {
        link: SecureLink,
    },
}

/// O endpoint em si.
pub struct Endpoint {
    socket: Arc<UdpSocket>,
    identity: Arc<Identity>,
    events: mpsc::UnboundedSender<NetEvent>,
    state: State,
}

impl core::fmt::Debug for Endpoint {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Endpoint").finish_non_exhaustive()
    }
}

/// Alças para falar com um endpoint em execução.
#[derive(Debug)]
pub struct EndpointHandle {
    /// Manda comandos ao endpoint.
    pub commands: mpsc::UnboundedSender<NetCommand>,
    /// Recebe eventos do endpoint.
    pub events: mpsc::UnboundedReceiver<NetEvent>,
}

impl Endpoint {
    /// Sobe o endpoint numa tarefa e devolve as alças para conversar com ele.
    #[must_use]
    pub fn spawn(socket: Arc<UdpSocket>, identity: Arc<Identity>) -> EndpointHandle {
        let (cmd_tx, cmd_rx) = mpsc::unbounded_channel();
        let (evt_tx, evt_rx) = mpsc::unbounded_channel();
        let endpoint = Self {
            socket,
            identity,
            events: evt_tx,
            state: State::Idle,
        };
        tokio::spawn(endpoint.run(cmd_rx));
        EndpointHandle {
            commands: cmd_tx,
            events: evt_rx,
        }
    }

    async fn run(mut self, mut commands: mpsc::UnboundedReceiver<NetCommand>) {
        let mut buf = [0u8; BUF];
        loop {
            tokio::select! {
                command = commands.recv() => {
                    match command {
                        Some(NetCommand::Shutdown) | None => break,
                        Some(command) => self.on_command(command).await,
                    }
                }
                received = self.socket.recv_from(&mut buf) => {
                    match received {
                        Ok((len, from)) => {
                            let datagram = buf.get(..len).unwrap_or(&[]).to_vec();
                            self.on_datagram(from, &datagram).await;
                        }
                        Err(error) => {
                            let _ = self.events.send(NetEvent::Error(error.to_string()));
                        }
                    }
                }
            }
        }
    }

    async fn on_command(&mut self, command: NetCommand) {
        match command {
            NetCommand::Connect { peer, mode } => self.connect(peer, mode).await,
            NetCommand::SendFrame(bytes) => self.send_frame(&bytes).await,
            NetCommand::ConfirmPairing(ok) => self.confirm_pairing(ok).await,
            NetCommand::Disconnect => self.tear_down("pedido local"),
            NetCommand::Shutdown => {}
        }
    }

    /// Conecta como iniciador.
    async fn connect(&mut self, peer: SocketAddr, mode: ConnectMode) {
        match handshake::drive_initiator(&self.socket, peer, &self.identity, mode).await {
            Ok(established) => self.on_established(peer, established),
            Err(error) => {
                let _ = self.events.send(NetEvent::Error(error.to_string()));
                self.tear_down("handshake falhou");
            }
        }
    }

    /// Trata um datagrama recebido, conforme a fase.
    async fn on_datagram(&mut self, from: SocketAddr, datagram: &[u8]) {
        match &mut self.state {
            State::Idle => self.maybe_respond(from, datagram).await,
            State::AwaitingConfirm { .. } | State::Established { .. } => {
                self.on_data_datagram(datagram);
            }
        }
    }

    /// Sem enlace: um datagrama de handshake vira uma resposta de respondedor.
    async fn maybe_respond(&mut self, from: SocketAddr, datagram: &[u8]) {
        if wire::parse_handshake(datagram).is_none() {
            return; // não é início de handshake; ignorado em silêncio
        }
        match handshake::drive_responder(&self.socket, from, &self.identity, datagram).await {
            Ok(established) => self.on_established(from, established),
            Err(error) => {
                let _ = self.events.send(NetEvent::Error(error.to_string()));
            }
        }
    }

    /// Um handshake terminou: ou pede confirmação (pareamento) ou já estabelece (reconexão).
    fn on_established(&mut self, peer: SocketAddr, established: handshake::Established) {
        let link = SecureLink::new(Arc::clone(&self.socket), peer, established.transport);
        let peer_static = established.peer_static;
        if let Some(code) = established.code {
            // Pareamento: mostra o código e espera a confirmação dos dois lados antes de
            // deixar qualquer quadro de sessão passar.
            let _ = self.events.send(NetEvent::PairingCode {
                code,
                peer_static,
                peer,
            });
            self.state = State::AwaitingConfirm {
                link,
                peer_static,
                local_ok: false,
                peer_ok: false,
            };
        } else {
            // Reconexão: a identidade já está fixada, então o enlace já vale.
            let _ = self
                .events
                .send(NetEvent::Established { peer_static, peer });
            self.state = State::Established { link };
        }
    }

    /// Abre um datagrama de dados e age conforme a espécie.
    fn on_data_datagram(&mut self, datagram: &[u8]) {
        let opened = match &mut self.state {
            State::AwaitingConfirm { link, .. } | State::Established { link } => {
                link.open(datagram)
            }
            State::Idle => return,
        };
        // Um datagrama que não abre é lixo, repetição, ou de outra sessão. Ignorado: derrubar
        // por um pacote solto abriria uma negação de serviço trivial.
        let Ok((kind, payload)) = opened else {
            return;
        };
        match kind {
            Kind::SessionFrame => self.deliver_frame(payload),
            Kind::PairConfirm => self.peer_confirmed(),
            Kind::PairReject => {
                let _ = self
                    .events
                    .send(NetEvent::LinkDown("o par recusou o pareamento"));
                self.state = State::Idle;
            }
        }
    }

    fn deliver_frame(&mut self, payload: Vec<u8>) {
        if matches!(self.state, State::Established { .. }) {
            let _ = self.events.send(NetEvent::Frame(payload));
        }
        // Quadro de sessão antes da confirmação do pareamento é descartado: nada de sessão
        // trafega antes das duas confirmações.
    }

    fn peer_confirmed(&mut self) {
        if let State::AwaitingConfirm { peer_ok, .. } = &mut self.state {
            *peer_ok = true;
        }
        self.promote_if_ready();
    }

    /// O usuário respondeu à comparação de códigos.
    async fn confirm_pairing(&mut self, ok: bool) {
        let State::AwaitingConfirm { link, local_ok, .. } = &mut self.state else {
            return;
        };
        if ok {
            *local_ok = true;
            let _ = link.send(Kind::PairConfirm, &[]).await;
            self.promote_if_ready();
        } else {
            let _ = link.send(Kind::PairReject, &[]).await;
            let _ = self.events.send(NetEvent::LinkDown("códigos diferentes"));
            self.state = State::Idle;
        }
    }

    /// Estabelece o enlace quando os dois lados confirmaram.
    fn promote_if_ready(&mut self) {
        let ready = matches!(
            &self.state,
            State::AwaitingConfirm {
                local_ok: true,
                peer_ok: true,
                ..
            }
        );
        if !ready {
            return;
        }
        let old = core::mem::replace(&mut self.state, State::Idle);
        if let State::AwaitingConfirm {
            link, peer_static, ..
        } = old
        {
            let peer = link.peer();
            let _ = self
                .events
                .send(NetEvent::Established { peer_static, peer });
            self.state = State::Established { link };
        }
    }

    async fn send_frame(&mut self, bytes: &[u8]) {
        if let State::Established { link } = &mut self.state
            && let Err(error) = link.send(Kind::SessionFrame, bytes).await
        {
            let _ = self.events.send(NetEvent::Error(error.to_string()));
        }
    }

    fn tear_down(&mut self, reason: &'static str) {
        if !matches!(self.state, State::Idle) {
            self.state = State::Idle;
            let _ = self.events.send(NetEvent::LinkDown(reason));
        }
    }
}

/// Vincula um socket UDP a uma porta, pronto para o endpoint.
///
/// # Errors
///
/// [`NetError::Io`] se a porta não puder ser vinculada.
pub async fn bind(addr: SocketAddr) -> Result<Arc<UdpSocket>> {
    let socket = UdpSocket::bind(addr).await.map_err(NetError::Io)?;
    Ok(Arc::new(socket))
}
