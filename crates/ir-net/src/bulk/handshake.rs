//! O handshake do canal de dados: `Noise_IK`, sempre, e a conferência da identidade fixada.
//!
//! # Por que só `IK`
//!
//! O canal de dados **nunca é o primeiro encontro**. Ele existe porque a sessão de entrada já
//! pareou, já mostrou o código de seis dígitos e já fixou a chave estática do par
//! ([01, §3.3](../../../docs/01-visao-e-escopo.md): "a mesma identidade da sessão"). Um caminho
//! `XX` aqui seria um segundo jeito de conhecer um par — e o segundo jeito é sempre o que não
//! recebe a mesma atenção.
//!
//! Consequência prática: não há byte de modo no fio, porque não há modo a escolher.
//!
//! # Quem confere o quê
//!
//! No `IK` o iniciador já conhece a chave do respondedor: ela entra no padrão, e se o
//! respondedor não a possuir o handshake simplesmente não fecha. O **respondedor**, ao
//! contrário, descobre a chave do iniciador no meio do handshake — e é ele que precisa
//! comparar com a fixada. Sem essa comparação, qualquer um que alcance a porta 52525 abriria um
//! canal de arquivos autenticado como "alguém".
//!
//! Os dois lados conferem, mesmo assim. Barato, e a simetria evita que a pergunta "deste lado
//! precisa?" volte a cada leitura.

use core::time::Duration;

use ir_crypto::enlace::concluir;
use ir_crypto::{Handshake, Identity, PublicKey};

use crate::bulk::link::BulkLink;
use crate::bulk::stream::{Channel, Frames};
use crate::error::{NetError, Result};

/// Quanto esperar por cada mensagem do par antes de desistir.
///
/// A conexão TCP já está de pé quando o handshake começa, então o que se espera aqui é só
/// processamento e um trecho de rede — não um par que talvez não exista.
const STEP_TIMEOUT: Duration = Duration::from_millis(1500);

/// Teto de passos do laço. `IK` tem duas mensagens; quatro cobre com folga e garante término.
const MAX_STEPS: usize = 4;

/// Abre o canal como quem disca.
///
/// # Errors
///
/// [`NetError::HandshakeTimeout`] se o par não responder a tempo; [`NetError::WrongPeer`] se ele
/// apresentar outra identidade; [`NetError::Crypto`] se o Noise recusar; [`NetError::Io`] em
/// falha de socket.
pub async fn dial<C: Channel>(
    frames: Frames<C>,
    identity: &Identity,
    peer: PublicKey,
) -> Result<BulkLink<C>> {
    let handshake = Handshake::reconnect_initiator(identity, peer)?;
    run(frames, handshake, peer).await
}

/// Abre o canal como quem atende.
///
/// # Errors
///
/// Os mesmos de [`dial`]. Em especial [`NetError::WrongPeer`]: é **aqui** que a identidade de
/// quem discou é conferida contra a fixada, e recusar é o comportamento correto.
pub async fn accept<C: Channel>(
    mut frames: Frames<C>,
    identity: &Identity,
    expected: PublicKey,
) -> Result<BulkLink<C>> {
    let mut handshake = Handshake::reconnect_responder(identity)?;
    let first = step(&mut frames).await?;
    handshake.read_message(&first)?;
    run(frames, handshake, expected).await
}

/// O laço comum: alterna escrever e ler até o handshake fechar, e então confere a identidade.
async fn run<C: Channel>(
    mut frames: Frames<C>,
    mut handshake: Handshake,
    expected: PublicKey,
) -> Result<BulkLink<C>> {
    for _ in 0..MAX_STEPS {
        if handshake.is_finished() {
            break;
        }
        if handshake.is_my_turn() {
            let message = handshake.write_message()?;
            frames.send_now(&message).await?;
        } else {
            let body = step(&mut frames).await?;
            handshake.read_message(&body)?;
        }
    }
    let established = concluir(handshake)?;
    if established.peer_static != expected {
        return Err(NetError::WrongPeer);
    }
    Ok(BulkLink::new(frames, established.transport))
}

/// Uma mensagem do par, com prazo.
async fn step<C: Channel>(frames: &mut Frames<C>) -> Result<Vec<u8>> {
    tokio::time::timeout(STEP_TIMEOUT, frames.recv())
        .await
        .map_err(|_| NetError::HandshakeTimeout)?
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Um par de identidades e o *stream* que as liga.
    fn bench() -> (
        Identity,
        Identity,
        tokio::io::DuplexStream,
        tokio::io::DuplexStream,
    ) {
        let (a, b) = tokio::io::duplex(crate::bulk::wire::MAX_BODY);
        (Identity::generate(), Identity::generate(), a, b)
    }

    #[tokio::test]
    async fn the_two_sides_open_a_link_and_talk() {
        let (here, there, a, b) = bench();
        let (chave_do_par, chave_nossa) = (there.public(), here.public());

        let atende = tokio::spawn(async move { accept(Frames::new(b), &there, chave_nossa).await });
        let mut disca = dial(Frames::new(a), &here, chave_do_par)
            .await
            .expect("o handshake tem de fechar");
        let mut atendido = atende.await.expect("a tarefa").expect("atende");

        disca.send_now(b"manifesto").await.unwrap();
        assert_eq!(atendido.recv().await.unwrap(), b"manifesto");
        // E no sentido de volta, que é o que carrega `Accept` e `Verified`.
        atendido.send_now(b"aceito").await.unwrap();
        assert_eq!(disca.recv().await.unwrap(), b"aceito");
    }

    #[tokio::test]
    async fn the_side_that_answers_refuses_an_identity_it_has_not_pinned() {
        // O caso que este módulo existe para barrar: alguém alcança a porta e disca com uma
        // identidade válida, só que não a fixada.
        let (here, there, a, b) = bench();
        let estranho = Identity::generate();
        let chave_do_par = there.public();

        let atende =
            tokio::spawn(async move { accept(Frames::new(b), &there, estranho.public()).await });
        let _ = dial(Frames::new(a), &here, chave_do_par).await;
        let recusa = atende.await.expect("a tarefa");
        assert!(
            matches!(recusa, Err(NetError::WrongPeer)),
            "esperava WrongPeer, veio {recusa:?}"
        );
    }

    #[tokio::test]
    async fn the_side_that_dials_fails_against_the_wrong_static_key() {
        // No `IK` a chave do respondedor entra no padrão: se ela não conferir, o handshake não
        // fecha — a falha vem da criptografia, antes de qualquer comparação nossa.
        let (here, there, a, b) = bench();
        let outra = Identity::generate().public();
        let chave_nossa = here.public();

        tokio::spawn(async move { accept(Frames::new(b), &there, chave_nossa).await });
        let erro = dial(Frames::new(a), &here, outra).await;
        assert!(erro.is_err(), "discar para a chave errada não pode passar");
    }

    #[tokio::test]
    async fn a_silent_peer_times_out_instead_of_hanging() {
        let (here, _there, a, b) = bench();
        let alvo = Identity::generate().public();
        // `b` fica vivo e calado: o prazo é o que tem de resolver.
        let guarda = b;
        let erro = dial(Frames::new(a), &here, alvo).await;
        assert!(matches!(erro, Err(NetError::HandshakeTimeout)));
        drop(guarda);
    }
}
