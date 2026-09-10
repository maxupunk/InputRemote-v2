//! O handshake Noise: `Noise_XX` para o primeiro pareamento, `Noise_IK` para reconectar.
//!
//! Um padrão para cada situação ([03, §3](../../../docs/03-protocolo.md)):
//!
//! - **XX** — as duas máquinas ainda não se conhecem. O handshake roda sem autenticação prévia,
//!   e ao final cada lado deriva um código de 6 dígitos do hash do handshake. O usuário compara
//!   os dois códigos nas duas telas; só então cada lado grava a chave estática do outro.
//! - **IK** — o iniciador já conhece a chave estática do respondedor e a apresenta fixada. Uma
//!   chave diferente é recusa, não pergunta.

use snow::{Builder, HandshakeState};

use crate::error::{CryptoError, Result};
use crate::identity::{Identity, PublicKey};
use crate::transport::Transport;

/// Os parâmetros do pareamento: `Noise_XX`.
fn pair_params() -> Result<snow::params::NoiseParams> {
    "Noise_XX_25519_ChaChaPoly_BLAKE2s"
        .parse()
        .map_err(|_| CryptoError::Handshake)
}

/// Os parâmetros da reconexão: `Noise_IK`.
fn reconnect_params() -> Result<snow::params::NoiseParams> {
    "Noise_IK_25519_ChaChaPoly_BLAKE2s"
        .parse()
        .map_err(|_| CryptoError::Handshake)
}

/// Espaço de buffer para uma mensagem de handshake. As mensagens do Noise são pequenas
/// (elementos de curva e tags), e 512 B cobre qualquer uma delas com folga.
const HANDSHAKE_BUF: usize = 512;

/// O handshake em andamento.
///
/// Depois de terminado, [`Handshake::into_transport`] devolve o [`Transport`] que cifra os
/// quadros. O SAS só faz sentido para o pareamento (XX); na reconexão a identidade já é fixada.
#[derive(Debug)]
pub struct Handshake {
    state: HandshakeState,
    my_turn: bool,
    is_pairing: bool,
}

impl Handshake {
    /// Inicia um pareamento (papel de iniciador).
    ///
    /// # Errors
    ///
    /// [`CryptoError::Handshake`] se o estado do Noise não puder ser montado.
    pub fn pair_initiator(identity: &Identity) -> Result<Self> {
        let state = Builder::new(pair_params()?)
            .local_private_key(identity.secret())
            .and_then(Builder::build_initiator)
            .map_err(|_| CryptoError::Handshake)?;
        Ok(Self {
            state,
            my_turn: true,
            is_pairing: true,
        })
    }

    /// Responde a um pareamento (papel de respondedor).
    ///
    /// # Errors
    ///
    /// [`CryptoError::Handshake`] se o estado do Noise não puder ser montado.
    pub fn pair_responder(identity: &Identity) -> Result<Self> {
        let state = Builder::new(pair_params()?)
            .local_private_key(identity.secret())
            .and_then(Builder::build_responder)
            .map_err(|_| CryptoError::Handshake)?;
        Ok(Self {
            state,
            my_turn: false,
            is_pairing: true,
        })
    }

    /// Reconecta a um par conhecido (papel de iniciador), fixando a chave dele.
    ///
    /// # Errors
    ///
    /// [`CryptoError::Handshake`] se o estado não puder ser montado.
    pub fn reconnect_initiator(identity: &Identity, peer: PublicKey) -> Result<Self> {
        let state = Builder::new(reconnect_params()?)
            .local_private_key(identity.secret())
            .and_then(|b| b.remote_public_key(&peer.0))
            .and_then(Builder::build_initiator)
            .map_err(|_| CryptoError::Handshake)?;
        Ok(Self {
            state,
            my_turn: true,
            is_pairing: false,
        })
    }

    /// Aceita a reconexão de um par (papel de respondedor).
    ///
    /// A chave do iniciador chega no handshake e sai por [`Handshake::remote_static`]; quem
    /// chama **deve** compará-la com a fixada e recusar se divergir.
    ///
    /// # Errors
    ///
    /// [`CryptoError::Handshake`] se o estado não puder ser montado.
    pub fn reconnect_responder(identity: &Identity) -> Result<Self> {
        let state = Builder::new(reconnect_params()?)
            .local_private_key(identity.secret())
            .and_then(Builder::build_responder)
            .map_err(|_| CryptoError::Handshake)?;
        Ok(Self {
            state,
            my_turn: false,
            is_pairing: false,
        })
    }

    /// Se é a vez desta ponta escrever a próxima mensagem.
    #[must_use]
    pub fn is_my_turn(&self) -> bool {
        self.my_turn && !self.is_finished()
    }

    /// Se o handshake terminou.
    #[must_use]
    pub fn is_finished(&self) -> bool {
        self.state.is_handshake_finished()
    }

    /// Produz a próxima mensagem de handshake a enviar ao par.
    ///
    /// # Errors
    ///
    /// [`CryptoError::OutOfTurn`] se não for a vez desta ponta; [`CryptoError::Handshake`] se o
    /// Noise recusar.
    pub fn write_message(&mut self) -> Result<Vec<u8>> {
        if !self.my_turn || self.is_finished() {
            return Err(CryptoError::OutOfTurn);
        }
        let mut buf = [0u8; HANDSHAKE_BUF];
        let len = self
            .state
            .write_message(&[], &mut buf)
            .map_err(|_| CryptoError::Handshake)?;
        self.my_turn = false;
        Ok(buf.get(..len).unwrap_or(&[]).to_vec())
    }

    /// Consome uma mensagem de handshake recebida do par.
    ///
    /// # Errors
    ///
    /// [`CryptoError::OutOfTurn`] se era a vez desta ponta escrever; [`CryptoError::Handshake`]
    /// se a mensagem for inválida ou adulterada.
    pub fn read_message(&mut self, message: &[u8]) -> Result<()> {
        if self.my_turn || self.is_finished() {
            return Err(CryptoError::OutOfTurn);
        }
        let mut buf = [0u8; HANDSHAKE_BUF];
        self.state
            .read_message(message, &mut buf)
            .map_err(|_| CryptoError::Handshake)?;
        self.my_turn = true;
        Ok(())
    }

    /// A chave estática que o par apresentou, se já foi trocada.
    ///
    /// No pareamento (XX) e na reconexão (IK respondedor), é o que se grava ou se confere.
    #[must_use]
    pub fn remote_static(&self) -> Option<PublicKey> {
        self.state.get_remote_static().and_then(|bytes| {
            let array: [u8; 32] = bytes.try_into().ok()?;
            Some(PublicKey(array))
        })
    }

    /// O código de 6 dígitos para o usuário comparar nas duas telas.
    ///
    /// Só existe para o pareamento; na reconexão a identidade já está fixada e não há o que
    /// comparar.
    ///
    /// # Errors
    ///
    /// [`CryptoError::NotFinished`] se o handshake ainda não terminou.
    pub fn pairing_code(&self) -> Result<[u8; 6]> {
        if !self.is_pairing {
            return Err(CryptoError::NotFinished);
        }
        if !self.is_finished() {
            return Err(CryptoError::NotFinished);
        }
        Ok(crate::sas::code_from_handshake_hash(
            self.state.get_handshake_hash(),
        ))
    }

    /// Termina o handshake e devolve o transporte cifrado.
    ///
    /// # Errors
    ///
    /// [`CryptoError::NotFinished`] se ainda não terminou; [`CryptoError::Handshake`] se o Noise
    /// recusar a transição.
    pub fn into_transport(self) -> Result<Transport> {
        if !self.state.is_handshake_finished() {
            return Err(CryptoError::NotFinished);
        }
        let stateless = self
            .state
            .into_stateless_transport_mode()
            .map_err(|_| CryptoError::Handshake)?;
        Ok(Transport::new(stateless))
    }
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used)]
mod tests {
    use super::*;

    /// Roda um handshake inteiro entre dois lados e devolve os dois handshakes terminados.
    fn drive(mut a: Handshake, mut b: Handshake) -> (Handshake, Handshake) {
        for _ in 0..8 {
            if a.is_finished() && b.is_finished() {
                break;
            }
            if a.is_my_turn() {
                let msg = a.write_message().expect("a escreve");
                b.read_message(&msg).expect("b lê");
            } else if b.is_my_turn() {
                let msg = b.write_message().expect("b escreve");
                a.read_message(&msg).expect("a lê");
            }
        }
        (a, b)
    }

    #[test]
    fn pairing_produces_the_same_code_on_both_sides() {
        let alice = Identity::generate();
        let bob = Identity::generate();
        let (a, b) = drive(
            Handshake::pair_initiator(&alice).unwrap(),
            Handshake::pair_responder(&bob).unwrap(),
        );
        assert!(a.is_finished() && b.is_finished());
        assert_eq!(a.pairing_code().unwrap(), b.pairing_code().unwrap());
        // E cada lado aprendeu a chave estática do outro.
        assert_eq!(a.remote_static(), Some(bob.public()));
        assert_eq!(b.remote_static(), Some(alice.public()));
    }

    #[test]
    fn a_man_in_the_middle_produces_different_codes() {
        let alice = Identity::generate();
        let mallory = Identity::generate();
        let bob = Identity::generate();
        // Mallory faz dois handshakes distintos, um com cada lado.
        let (a, m1) = drive(
            Handshake::pair_initiator(&alice).unwrap(),
            Handshake::pair_responder(&mallory).unwrap(),
        );
        let (m2, b) = drive(
            Handshake::pair_initiator(&mallory).unwrap(),
            Handshake::pair_responder(&bob).unwrap(),
        );
        // Os códigos que Alice e Bob veem vêm de hashes diferentes.
        assert_ne!(a.pairing_code().unwrap(), b.pairing_code().unwrap());
        let _ = (m1, m2);
    }

    #[test]
    fn reconnect_learns_the_initiators_key_and_pins_the_responders() {
        let client = Identity::generate();
        let server = Identity::generate();
        let (c, s) = drive(
            Handshake::reconnect_initiator(&client, server.public()).unwrap(),
            Handshake::reconnect_responder(&server).unwrap(),
        );
        assert!(c.is_finished() && s.is_finished());
        // O respondedor precisa conferir a chave que chegou contra a fixada.
        assert_eq!(s.remote_static(), Some(client.public()));
        let _ = c;
    }

    #[test]
    fn a_finished_pairing_yields_working_transports() {
        let alice = Identity::generate();
        let bob = Identity::generate();
        let (a, b) = drive(
            Handshake::pair_initiator(&alice).unwrap(),
            Handshake::pair_responder(&bob).unwrap(),
        );
        let mut ta = a.into_transport().unwrap();
        let mut tb = b.into_transport().unwrap();
        let (counter, sealed) = ta.seal(b"segredo").unwrap();
        assert_eq!(tb.open(counter, &sealed).unwrap(), b"segredo");
    }

    #[test]
    fn the_noise_parameter_strings_parse() {
        assert!(pair_params().is_ok());
        assert!(reconnect_params().is_ok());
    }
}
