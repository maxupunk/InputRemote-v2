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
pub use crate::vocabulario::{NetCommand, NetEvent};

mod rechave;
use crate::wire::{self, Kind, Mode};

/// Buffer de recepção. Cobre o maior datagrama do produto (1200 B de texto claro + folga).
const BUF: usize = 2048;

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
        /// Quem está do outro lado. Um reinício do enlace só é aceito com a mesma chave.
        peer_static: PublicKey,
    },
}

/// O endpoint em si.
pub struct Endpoint {
    socket: Arc<UdpSocket>,
    identity: Arc<Identity>,
    events: mpsc::UnboundedSender<NetEvent>,
    state: State,
    /// Rodadas de reconexão desde o último enlace, para a regra de [`crate::turno`].
    rodadas: u32,
    /// Se um pareamento que chega de fora é atendido ([`NetCommand::AcceptPairing`]).
    aceitar_pareamento: bool,
    /// Quando saiu a última tentativa de trocar as chaves ([`rechave`]).
    rechave_tentada: Option<std::time::Instant>,
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
            rodadas: 0,
            aceitar_pareamento: true,
            rechave_tentada: None,
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
                        // O ICMP de "porta inalcançável" de um envio anterior (ver `handshake`):
                        // não é erro deste socket, e virava uma linha de aviso a cada tentativa.
                        Err(error) if error.kind() == std::io::ErrorKind::ConnectionReset => {}
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
            NetCommand::AcceptPairing(aceitar) => self.aceitar_pareamento = aceitar,
            #[cfg(test)]
            NetCommand::ForcarRechave => {
                if let State::Established { link, .. } = &mut self.state {
                    link.pedir_rechave();
                }
            }
            NetCommand::Disconnect => self.tear_down("pedido local"),
            NetCommand::Shutdown => {}
        }
    }

    /// Conecta como iniciador — na reconexão, só se for a vez deste lado ([`crate::turno`]).
    async fn connect(&mut self, peer: SocketAddr, mode: ConnectMode) {
        if let ConnectMode::Reconnect(peer_key) = mode {
            self.rodadas = self.rodadas.wrapping_add(1);
            if !crate::turno::discar_nesta_rodada(self.identity.public(), peer_key, self.rodadas) {
                return;
            }
        }
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
            State::AwaitingConfirm { .. } => {
                self.on_data_datagram(datagram);
            }
            State::Established { .. } => {
                if !self.on_data_datagram(datagram) && !self.atender_rechave(from, datagram).await {
                    self.maybe_restart(from, datagram).await;
                }
            }
        }
    }

    /// O par recomeçou o enlace enquanto o nosso ainda estava de pé.
    ///
    /// Acontece quando ele reiniciou, ou perdeu o enlace por um motivo que daqui não se viu. Sem
    /// isto, este lado continuava num enlace que do outro lado já não existia — reabrindo sessão
    /// sobre ele a cada rodada — e tratava o handshake do par como dado inválido; o par discava
    /// para sempre. Foi o que a bancada mostrou depois de uma queda.
    ///
    /// Só troca de enlace se o handshake novo terminar **com a mesma chave**: um datagrama forjado
    /// com o endereço do par, de outra identidade, não derruba o enlace que funciona.
    async fn maybe_restart(&mut self, from: SocketAddr, datagram: &[u8]) {
        let State::Established { link, peer_static } = &self.state else {
            return;
        };
        let reinicio = matches!(wire::parse_handshake(datagram), Some((Mode::Reconnect, _)));
        if from != link.peer() || !reinicio {
            return;
        }
        let atual = *peer_static;
        // Falha aqui é lixo ou repetição de um enlace velho; o enlace de agora continua valendo.
        let Ok(novo) =
            handshake::drive_responder(&self.socket, from, &self.identity, datagram).await
        else {
            return;
        };
        if novo.peer_static != atual {
            return;
        }
        let _ = self
            .events
            .send(NetEvent::LinkDown("o par recomeçou o enlace"));
        self.state = State::Idle;
        self.on_established(from, novo);
    }

    /// Sem enlace: um datagrama de handshake vira uma resposta de respondedor.
    async fn maybe_respond(&mut self, from: SocketAddr, datagram: &[u8]) {
        let Some((mode, _)) = wire::parse_handshake(datagram) else {
            return; // não é início de handshake; ignorado em silêncio
        };
        if mode == Mode::Pair && !self.aceitar_pareamento {
            // Antes de qualquer criptografia: recusar custa um byte lido.
            tracing::debug!(%from, "pedido de pareamento ignorado: a janela de pareamento não está aberta");
            return;
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
        self.rodadas = 0;
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
            self.state = State::Established { link, peer_static };
        }
    }

    /// Abre um datagrama de dados e age conforme a espécie. `false` se ele não abriu.
    fn on_data_datagram(&mut self, datagram: &[u8]) -> bool {
        let opened = match &mut self.state {
            State::AwaitingConfirm { link, .. } | State::Established { link, .. } => {
                link.open(datagram)
            }
            State::Idle => return false,
        };
        // Um datagrama que não abre é lixo, repetição, ou de outra sessão. Ignorado: derrubar
        // por um pacote solto abriria uma negação de serviço trivial.
        let Ok((kind, payload)) = opened else {
            return false;
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
        true
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
            self.state = State::Established { link, peer_static };
        }
    }

    async fn send_frame(&mut self, bytes: &[u8]) {
        if let State::Established { link, .. } = &mut self.state
            && let Err(error) = link.send(Kind::SessionFrame, bytes).await
        {
            let _ = self.events.send(NetEvent::Error(error.to_string()));
        }
        self.rechavear_se_preciso().await;
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
