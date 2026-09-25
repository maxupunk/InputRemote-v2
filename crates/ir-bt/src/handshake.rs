//! O handshake Noise conduzido sobre um canal RFCOMM.
//!
//! A ponte entre o [`ir_crypto::Handshake`] — que só transforma bytes em bytes — e o canal de
//! verdade. É o mesmo handshake do UDP, com a mesma criptografia: `Noise_XX` com código de seis
//! dígitos no primeiro encontro, `Noise_IK` com a chave fixada depois.
//!
//! # O que o *stream* simplifica
//!
//! No UDP o respondedor descobre que há um handshake acontecendo ao receber um datagrama solto,
//! e precisa decidir o que fazer com ele. Aqui a conexão vem antes: quando há canal, há par do
//! outro lado, e o respondedor simplesmente espera o primeiro corpo. Não existe "datagrama de
//! outra origem no meio do handshake" para filtrar.

use core::time::Duration;

use ir_crypto::enlace::concluir;
pub use ir_crypto::enlace::{ConnectMode, Established};
use ir_crypto::{Handshake, Identity};

use crate::canal::{Canal, Quadros};
use crate::error::{BtError, Result};
use crate::wire::{self, Mode};

/// Quanto esperar por cada mensagem do par antes de desistir.
///
/// Maior que o do UDP (1,5 s) porque o rádio tem latência de estabelecimento maior e mais
/// variável que a rede local. Ainda assim cabe no critério de reconexão em menos de 5 s da
/// PoC-2: o `Noise_IK` da reconexão são duas mensagens, não seis.
const PRAZO_DE_PASSO: Duration = Duration::from_secs(2);

/// Teto de passos. Cobre `XX` (três mensagens) e `IK` (duas) com folga.
const MAX_PASSOS: usize = 6;

/// Conduz o handshake como iniciador.
///
/// # Errors
///
/// [`BtError::HandshakeTimeout`] se o par não responder a tempo; [`BtError::Crypto`] se a
/// criptografia recusar; [`BtError::Io`] em falha de socket; [`BtError::Malformed`] se pedirem
/// uma troca de chaves, que o rádio não faz.
pub async fn conduzir_iniciador<C: Canal>(
    quadros: &mut Quadros<C>,
    identidade: &Identity,
    modo: ConnectMode,
) -> Result<Established> {
    if matches!(modo, ConnectMode::Rekey(_)) {
        // A troca de chaves é só da rede; um enlace de rádio velho cai e é rediscado.
        return Err(BtError::Malformed);
    }
    let handshake = modo.iniciar(identidade)?;
    conduzir(quadros, handshake, modo.modo()).await
}

/// Espera o primeiro corpo de quem ligou, e lê dele o modo.
///
/// Separado de [`responder`] para quem atende poder recusar **antes** de qualquer criptografia:
/// o modo vem em claro justamente para isso.
///
/// # Errors
///
/// [`BtError::HandshakeTimeout`] se o par emudecer; [`BtError::Malformed`] se o corpo não for um
/// início de handshake — inclusive um pedido de troca de chaves, que o rádio não conhece.
pub async fn esperar_inicio<C: Canal>(quadros: &mut Quadros<C>) -> Result<(Mode, Vec<u8>)> {
    let primeiro = esperar(quadros).await?;
    match wire::ler_handshake(&primeiro) {
        Some((modo, mensagem)) if modo != Mode::Rekey => Ok((modo, mensagem.to_vec())),
        _ => Err(BtError::Malformed),
    }
}

/// Conduz o resto do handshake como respondedor, a partir do início já lido.
///
/// # Errors
///
/// Como [`conduzir_iniciador`].
pub async fn responder<C: Canal>(
    quadros: &mut Quadros<C>,
    identidade: &Identity,
    modo: Mode,
    mensagem: &[u8],
) -> Result<Established> {
    let mut handshake = modo.responder(identidade)?;
    handshake.read_message(mensagem)?;
    conduzir(quadros, handshake, modo).await
}

/// Conduz o handshake como respondedor, esperando o par falar primeiro.
///
/// O modo vem em claro no primeiro byte: é o que permite escolher o padrão Noise antes de
/// decifrar qualquer coisa.
///
/// # Errors
///
/// Como [`conduzir_iniciador`], mais [`BtError::Malformed`] se o primeiro corpo não for um
/// handshake válido.
pub async fn conduzir_respondedor<C: Canal>(
    quadros: &mut Quadros<C>,
    identidade: &Identity,
) -> Result<Established> {
    let (modo, mensagem) = esperar_inicio(quadros).await?;
    responder(quadros, identidade, modo, &mensagem).await
}

/// O laço comum: alterna escrever e ler até o handshake terminar.
async fn conduzir<C: Canal>(
    quadros: &mut Quadros<C>,
    mut handshake: Handshake,
    modo: Mode,
) -> Result<Established> {
    for _ in 0..MAX_PASSOS {
        if handshake.is_finished() {
            break;
        }
        if handshake.is_my_turn() {
            let mensagem = handshake.write_message()?;
            quadros
                .enviar(&wire::corpo_de_handshake(modo, &mensagem))
                .await?;
        } else {
            let corpo = esperar(quadros).await?;
            let (_, mensagem) = wire::ler_handshake(&corpo).ok_or(BtError::Malformed)?;
            handshake.read_message(mensagem)?;
        }
    }
    Ok(concluir(handshake)?)
}

/// Espera um corpo do par, com prazo.
///
/// Sem prazo, um par que abre o canal e emudece deixaria o handshake esperando para sempre — e
/// quem espera handshake não tenta reconectar.
async fn esperar<C: Canal>(quadros: &mut Quadros<C>) -> Result<Vec<u8>> {
    tokio::time::timeout(PRAZO_DE_PASSO, quadros.receber())
        .await
        .map_err(|_| BtError::HandshakeTimeout)?
}

#[cfg(test)]
mod tests {
    use tokio::io::DuplexStream;

    use super::*;

    fn canais() -> (Quadros<DuplexStream>, Quadros<DuplexStream>) {
        let (a, b) = tokio::io::duplex(8192);
        (Quadros::novo(a), Quadros::novo(b))
    }

    /// Roda os dois lados ao mesmo tempo, que é como um handshake acontece.
    async fn apertar(
        modo: ConnectMode,
        ia: Identity,
        ib: Identity,
    ) -> (Result<Established>, Result<Established>) {
        let (mut aqui, mut la) = canais();
        tokio::join!(
            async move { conduzir_iniciador(&mut aqui, &ia, modo).await },
            async move { conduzir_respondedor(&mut la, &ib).await }
        )
    }

    #[tokio::test]
    async fn o_pareamento_termina_com_o_mesmo_codigo_dos_dois_lados() {
        // O código é o que o usuário compara nas duas telas. Se os dois lados derivarem números
        // diferentes de um handshake legítimo, o produto acusa ataque onde não há.
        let (ia, ib) = (Identity::generate(), Identity::generate());
        let (publica_a, publica_b) = (ia.public(), ib.public());
        let (iniciador, respondedor) = apertar(ConnectMode::Pair, ia, ib).await;

        let iniciador = iniciador.expect("o iniciador termina");
        let respondedor = respondedor.expect("o respondedor termina");
        let codigo = iniciador.code.expect("pareamento tem código");
        assert_eq!(Some(codigo), respondedor.code);
        assert_eq!(codigo.len(), 6);
        assert!(codigo.iter().all(|d| *d <= 9), "seis dígitos decimais");

        // E cada lado ficou com a chave estática do outro, que é o que será fixado.
        assert_eq!(iniciador.peer_static, publica_b);
        assert_eq!(respondedor.peer_static, publica_a);
    }

    #[tokio::test]
    async fn a_reconexao_nao_mostra_codigo_nenhum() {
        // `Noise_IK`: a identidade já está fixada, e não há nada para o usuário comparar. Pedir
        // confirmação de novo a cada reconexão seria treinar o usuário a clicar sem olhar.
        let (ia, ib) = (Identity::generate(), Identity::generate());
        let chave_de_b = ib.public();
        let (iniciador, respondedor) = apertar(ConnectMode::Reconnect(chave_de_b), ia, ib).await;

        assert!(iniciador.expect("iniciador").code.is_none());
        assert!(respondedor.expect("respondedor").code.is_none());
    }

    #[tokio::test]
    async fn o_transporte_que_sai_do_handshake_ja_conversa() {
        // O handshake não serve de nada se o transporte que ele produz não abrir o quadro do
        // outro lado. Este é o teste que liga as duas metades.
        let (ia, ib) = (Identity::generate(), Identity::generate());
        let (iniciador, respondedor) = apertar(ConnectMode::Pair, ia, ib).await;
        let mut ta = iniciador.expect("iniciador").transport;
        let mut tb = respondedor.expect("respondedor").transport;

        let (contador, cifrado) = ta.seal(b"ctrl+alt+del").expect("cifra");
        assert_eq!(
            tb.open(contador, &cifrado).expect("decifra"),
            b"ctrl+alt+del"
        );
    }

    #[tokio::test]
    async fn reconectar_com_a_chave_errada_e_recusado() {
        // "Confiar na primeira vez" não existe depois do pareamento (docs/03 §3). Uma chave
        // fixada que não confere precisa derrubar o handshake, não abrir sessão.
        let (ia, ib, intrusa) = (
            Identity::generate(),
            Identity::generate(),
            Identity::generate(),
        );
        let (iniciador, respondedor) =
            apertar(ConnectMode::Reconnect(intrusa.public()), ia, ib).await;
        assert!(
            iniciador.is_err() || respondedor.is_err(),
            "um handshake IK contra outra chave não pode terminar bem"
        );
    }

    #[tokio::test]
    async fn um_par_que_emudece_vence_no_prazo() {
        // Sem prazo, um canal aberto e silencioso prenderia o serviço para sempre — e quem
        // espera handshake não tenta reconectar.
        let (mut aqui, _mudo) = canais();
        let identidade = Identity::generate();
        let erro = conduzir_iniciador(&mut aqui, &identidade, ConnectMode::Pair)
            .await
            .expect_err("precisa desistir");
        assert!(matches!(erro, BtError::HandshakeTimeout), "{erro}");
    }

    #[tokio::test]
    async fn lixo_no_lugar_do_handshake_e_recusado() {
        // Bytes de qualquer um, vindos do rádio, antes de haver criptografia.
        let (mut bruto, la) = canais();
        let identidade = Identity::generate();
        let (_, respondedor) = tokio::join!(
            async move {
                // Um corpo cujo primeiro byte não é um modo conhecido.
                bruto.enviar(&[0xEE, 0x01, 0x02]).await
            },
            async move {
                let mut la = la;
                conduzir_respondedor(&mut la, &identidade).await
            }
        );
        assert!(matches!(
            respondedor.expect_err("precisa recusar"),
            BtError::Malformed
        ));
    }
}
