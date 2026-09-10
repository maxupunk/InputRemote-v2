//! O transporte cifrado, depois do handshake.
//!
//! Cifra e decifra quadros com o contador explícito de [03, §3.1](../../../docs/03-protocolo.md).
//! O contador é o nonce do Noise; do lado de quem recebe, a [`ReplayWindow`] rejeita repetição e
//! quadros antigos demais.
//!
//! Funciona igual para os três portadores: sobre stream (RFCOMM, TCP) o contador chega em ordem
//! e a janela quase nunca precisa reordenar; sobre datagrama (UDP) é ela que segura a garantia.

use snow::StatelessTransportState;

use crate::error::{CryptoError, Result};
use crate::replay::ReplayWindow;

/// Depois de quantas mensagens convém rechavear.
///
/// `2^20`, o teto de [03, §3](../../../docs/03-protocolo.md). Quem usa consulta
/// [`Transport::should_rekey`] e conduz um handshake novo; o transporte não força sozinho.
const REKEY_AFTER: u64 = 1 << 20;

/// Espaço extra que o Noise acrescenta a cada mensagem: a tag de 16 bytes.
const TAG: usize = 16;

/// O transporte cifrado de uma sessão.
pub struct Transport {
    state: StatelessTransportState,
    send_counter: u64,
    replay: ReplayWindow,
}

impl core::fmt::Debug for Transport {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Transport")
            .field("send_counter", &self.send_counter)
            .finish_non_exhaustive()
    }
}

impl Transport {
    pub(crate) fn new(state: StatelessTransportState) -> Self {
        Self {
            state,
            send_counter: 0,
            replay: ReplayWindow::new(),
        }
    }

    /// Cifra um quadro, devolvendo o contador usado e o texto cifrado.
    ///
    /// O contador começa em 1 e cresce de um em um; ele viaja junto do quadro para o outro lado
    /// poder decifrar e conferir contra a janela de repetição.
    ///
    /// # Errors
    ///
    /// [`CryptoError::Open`] se o Noise recusar a cifragem (não deve acontecer com buffer
    /// suficiente).
    pub fn seal(&mut self, plaintext: &[u8]) -> Result<(u64, Vec<u8>)> {
        let counter = self.send_counter.wrapping_add(1);
        self.send_counter = counter;
        let mut out = vec![0u8; plaintext.len() + TAG];
        let len = self
            .state
            .write_message(counter, plaintext, &mut out)
            .map_err(|_| CryptoError::Open)?;
        out.truncate(len);
        Ok((counter, out))
    }

    /// Decifra um quadro recebido, conferindo o contador contra a janela de repetição.
    ///
    /// # Errors
    ///
    /// [`CryptoError::Replay`] se o contador já foi visto ou é antigo demais;
    /// [`CryptoError::Open`] se a tag não confere (bytes adulterados ou de outra sessão).
    pub fn open(&mut self, counter: u64, ciphertext: &[u8]) -> Result<Vec<u8>> {
        // A janela é consultada **antes** de decifrar: um contador repetido não deve nem chegar
        // ao Noise. Mas a marcação como visto só acontece depois de a tag conferir, senão um
        // quadro forjado com contador novo "queimaria" aquele número para o quadro legítimo.
        if !self.replay.would_accept(counter) {
            return Err(CryptoError::Replay);
        }
        let mut out = vec![0u8; ciphertext.len()];
        let len = self
            .state
            .read_message(counter, ciphertext, &mut out)
            .map_err(|_| CryptoError::Open)?;
        // A tag conferiu: agora sim o contador é consumido.
        let _ = self.replay.accept(counter);
        out.truncate(len);
        Ok(out)
    }

    /// Se já convém rechavear, por volume de mensagens enviadas.
    #[must_use]
    pub const fn should_rekey(&self) -> bool {
        self.send_counter >= REKEY_AFTER
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use crate::handshake::Handshake;
    use crate::identity::Identity;

    fn paired() -> (super::Transport, super::Transport) {
        let a = Identity::generate();
        let b = Identity::generate();
        let mut ha = Handshake::pair_initiator(&a).unwrap();
        let mut hb = Handshake::pair_responder(&b).unwrap();
        for _ in 0..6 {
            if ha.is_finished() && hb.is_finished() {
                break;
            }
            if ha.is_my_turn() {
                let m = ha.write_message().unwrap();
                hb.read_message(&m).unwrap();
            } else if hb.is_my_turn() {
                let m = hb.write_message().unwrap();
                ha.read_message(&m).unwrap();
            }
        }
        (ha.into_transport().unwrap(), hb.into_transport().unwrap())
    }

    #[test]
    fn a_sealed_frame_opens_to_the_same_bytes() {
        let (mut a, mut b) = paired();
        let (c, sealed) = a.seal(b"ctrl+alt+del").unwrap();
        assert_eq!(b.open(c, &sealed).unwrap(), b"ctrl+alt+del");
    }

    #[test]
    fn a_replayed_frame_is_rejected() {
        let (mut a, mut b) = paired();
        let (c, sealed) = a.seal(b"tecla").unwrap();
        assert!(b.open(c, &sealed).is_ok());
        assert_eq!(
            b.open(c, &sealed).unwrap_err(),
            crate::error::CryptoError::Replay,
            "o mesmo quadro não pode ser aceito de novo"
        );
    }

    #[test]
    fn tampered_ciphertext_does_not_open_and_does_not_burn_the_counter() {
        let (mut a, mut b) = paired();
        let (c, good) = a.seal(b"senha").unwrap();
        let mut tampered = good.clone();
        tampered[0] ^= 0xFF;
        // O quadro adulterado, com o mesmo contador, falha na tag.
        assert_eq!(
            b.open(c, &tampered).unwrap_err(),
            crate::error::CryptoError::Open
        );
        // E como a tag não conferiu, o contador não foi consumido: o quadro legítimo ainda passa.
        assert_eq!(b.open(c, &good).unwrap(), b"senha");
    }

    #[test]
    fn out_of_order_delivery_still_opens() {
        let (mut a, mut b) = paired();
        let (c1, f1) = a.seal(b"um").unwrap();
        let (c2, f2) = a.seal(b"dois").unwrap();
        // Chegam trocados, como pode acontecer no UDP.
        assert_eq!(b.open(c2, &f2).unwrap(), b"dois");
        assert_eq!(b.open(c1, &f1).unwrap(), b"um");
    }

    #[test]
    fn the_wrong_counter_fails_to_open() {
        let (mut a, mut b) = paired();
        let (c, sealed) = a.seal(b"x").unwrap();
        assert!(
            b.open(c + 1, &sealed).is_err(),
            "contador errado é nonce errado"
        );
    }
}
