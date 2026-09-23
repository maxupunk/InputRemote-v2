//! A troca de chaves de um enlace que está de pé.
//!
//! Depois de 2^20 quadros com as mesmas chaves ([03, §3](../../../docs/03-protocolo.md)), quem envia
//! refaz o handshake — `Noise_IK`, com a chave do par que já está fixada — e troca o enlace **sem
//! derrubar a sessão**: nenhum aviso de queda sai de nenhum dos dois lados. O contador existia, e
//! nada o consultava.
//!
//! Um par que não conhece o modo de troca ignora o datagrama. O handshake daqui vence o prazo, o
//! enlace atual segue valendo, e a próxima tentativa só vem depois de [`ESPERA_ENTRE_TENTATIVAS`].

use std::net::SocketAddr;
use std::time::{Duration, Instant};

use ir_crypto::PublicKey;
use tracing::{debug, info};

use super::{Endpoint, State};
use crate::handshake::{self, ConnectMode, Established};
use crate::link::SecureLink;
use crate::wire::{self, Mode};

/// Quanto se espera para tentar de novo quando o par não respondeu à troca.
pub(super) const ESPERA_ENTRE_TENTATIVAS: Duration = Duration::from_secs(60);

/// A idade máxima das chaves de um enlace, pelo que vier primeiro com o contador
/// ([03, §3.1](../../../docs/03-protocolo.md)).
pub(super) const IDADE_MAXIMA: Duration = Duration::from_secs(10 * 60);

/// Se é este lado quem troca as chaves por idade.
///
/// Só um dos dois, e sempre o mesmo: as duas pontas fazem dez minutos quase juntas, e dois pedidos
/// de troca cruzados se atropelariam — cada um esperando a resposta do seu, os dois vencendo o
/// prazo, e de novo um minuto depois. Quem tem a menor chave pública pede; a outra ponta atende.
pub(super) fn troca_por_idade(minha: &PublicKey, do_par: &PublicKey, idade: Duration) -> bool {
    idade >= IDADE_MAXIMA && minha.0 < do_par.0
}

impl Endpoint {
    /// Troca as chaves se o enlace pedir, e se não houve tentativa recente.
    pub(super) async fn rechavear_se_preciso(&mut self) {
        let State::Established { link, peer_static } = &self.state else {
            return;
        };
        let por_idade = troca_por_idade(&self.identity.public(), peer_static, link.idade());
        if !(link.should_rekey() || por_idade)
            || self
                .rechave_tentada
                .is_some_and(|quando| quando.elapsed() < ESPERA_ENTRE_TENTATIVAS)
        {
            return;
        }
        let (peer, chave) = (link.peer(), *peer_static);
        self.rechave_tentada = Some(Instant::now());
        let resultado = handshake::drive_initiator(
            &self.socket,
            peer,
            &self.identity,
            ConnectMode::Rekey(chave),
        )
        .await;
        match resultado {
            Ok(novo) => self.trocar_em_silencio(peer, chave, novo),
            Err(erro) => debug!(%erro, "o par não trocou as chaves; o enlace atual segue"),
        }
    }

    /// O par pediu a troca de chaves do enlace de pé. Devolve `true` se o datagrama era isso.
    pub(super) async fn atender_rechave(&mut self, from: SocketAddr, datagram: &[u8]) -> bool {
        let State::Established { link, peer_static } = &self.state else {
            return false;
        };
        let pedido = matches!(wire::parse_handshake(datagram), Some((Mode::Rekey, _)));
        if from != link.peer() || !pedido {
            return false;
        }
        let chave = *peer_static;
        if let Ok(novo) =
            handshake::drive_responder(&self.socket, from, &self.identity, datagram).await
        {
            self.trocar_em_silencio(from, chave, novo);
        }
        true
    }

    /// Troca o enlace pelo novo, se ele terminou com a mesma chave — sem avisar ninguém.
    ///
    /// Outra chave seria outra máquina se fazendo passar pelo par: o enlace atual continua.
    fn trocar_em_silencio(&mut self, peer: SocketAddr, chave: PublicKey, novo: Established) {
        if novo.peer_static != chave {
            return;
        }
        info!("chaves do enlace trocadas");
        let link = SecureLink::new(std::sync::Arc::clone(&self.socket), peer, novo.transport);
        self.state = State::Established {
            link,
            peer_static: chave,
        };
        self.rechave_tentada = None;
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use std::sync::Arc;

    use ir_crypto::Identity;
    use tokio::sync::mpsc::UnboundedReceiver;

    use crate::{ConnectMode, Endpoint, EndpointHandle, NetCommand, NetEvent, bind};

    async fn proximo(eventos: &mut UnboundedReceiver<NetEvent>) -> NetEvent {
        tokio::time::timeout(std::time::Duration::from_secs(10), eventos.recv())
            .await
            .expect("o evento chega")
            .expect("o endpoint está vivo")
    }

    /// Dois endpoints em loopback, pareados e com o enlace de pé.
    async fn pareados() -> (EndpointHandle, EndpointHandle) {
        let socket_de_bob = bind("127.0.0.1:0".parse().unwrap()).await.unwrap();
        let bob_addr = socket_de_bob.local_addr().unwrap();
        let mut bob = Endpoint::spawn(socket_de_bob, Arc::new(Identity::generate()));
        let mut alice = Endpoint::spawn(
            bind("127.0.0.1:0".parse().unwrap()).await.unwrap(),
            Arc::new(Identity::generate()),
        );
        alice
            .commands
            .send(NetCommand::Connect {
                peer: bob_addr,
                mode: ConnectMode::Pair,
            })
            .unwrap();
        assert!(matches!(
            proximo(&mut alice.events).await,
            NetEvent::PairingCode { .. }
        ));
        assert!(matches!(
            proximo(&mut bob.events).await,
            NetEvent::PairingCode { .. }
        ));
        alice
            .commands
            .send(NetCommand::ConfirmPairing(true))
            .unwrap();
        bob.commands.send(NetCommand::ConfirmPairing(true)).unwrap();
        assert!(matches!(
            proximo(&mut alice.events).await,
            NetEvent::Established { .. }
        ));
        assert!(matches!(
            proximo(&mut bob.events).await,
            NetEvent::Established { .. }
        ));
        (alice, bob)
    }

    #[tokio::test]
    async fn a_troca_de_chaves_nao_derruba_o_enlace() {
        let (mut alice, mut bob) = pareados().await;

        alice.commands.send(NetCommand::ForcarRechave).unwrap();
        alice
            .commands
            .send(NetCommand::SendFrame(b"antes".to_vec()))
            .unwrap();
        assert!(matches!(proximo(&mut bob.events).await, NetEvent::Frame(q) if q == b"antes"));

        // Com as chaves novas, o quadro seguinte passa — e nenhum lado viu queda. Chegar antes do
        // prazo de um passo de handshake (1,5 s) é a prova de que a troca terminou: se o par não
        // tivesse respondido, o endpoint estaria preso esperando por ele.
        let inicio = std::time::Instant::now();
        alice
            .commands
            .send(NetCommand::SendFrame(b"depois".to_vec()))
            .unwrap();
        let chegou = proximo(&mut bob.events).await;
        assert!(inicio.elapsed() < std::time::Duration::from_secs(1));
        assert!(
            matches!(&chegou, NetEvent::Frame(q) if q == b"depois"),
            "{chegou:?}"
        );
        bob.commands
            .send(NetCommand::SendFrame(b"volta".to_vec()))
            .unwrap();
        let voltou = proximo(&mut alice.events).await;
        assert!(
            matches!(&voltou, NetEvent::Frame(q) if q == b"volta"),
            "{voltou:?}"
        );
    }

    #[test]
    fn por_idade_so_a_menor_chave_pede_e_so_depois_de_dez_minutos() {
        use super::{IDADE_MAXIMA, troca_por_idade};
        use ir_crypto::PublicKey;
        let (menor, maior) = (PublicKey([1; 32]), PublicKey([2; 32]));
        assert!(troca_por_idade(&menor, &maior, IDADE_MAXIMA));
        assert!(
            !troca_por_idade(&maior, &menor, IDADE_MAXIMA),
            "a outra ponta só atende"
        );
        assert!(!troca_por_idade(&menor, &maior, IDADE_MAXIMA / 2));
    }
}
