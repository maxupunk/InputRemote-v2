//! O transporte cifrado, depois do handshake.
//!
//! Cifra e decifra quadros com o contador explícito de [03, §3.1](../../../docs/03-protocolo.md).
//! O contador é o nonce do Noise; do lado de quem recebe, a [`ReplayWindow`] rejeita repetição e
//! quadros antigos demais.
//!
//! Funciona igual para os três portadores: sobre stream (RFCOMM, TCP) o contador chega em ordem
//! e a janela quase nunca precisa reordenar; sobre datagrama (UDP) é ela que segura a garantia.

use std::sync::Arc;

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

/// A metade que cifra: o estado do Noise e o contador de envio.
///
/// Ver [`Transport::split`] para o porquê de as duas metades poderem viver separadas.
pub struct Sealer {
    state: Arc<StatelessTransportState>,
    counter: u64,
}

impl core::fmt::Debug for Sealer {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Sealer")
            .field("counter", &self.counter)
            .finish_non_exhaustive()
    }
}

impl Sealer {
    /// Cifra um quadro, devolvendo o contador usado e o texto cifrado.
    ///
    /// O contador começa em 1 e cresce de um em um. Nos portadores de datagrama ele viaja junto
    /// do quadro; nos de stream as duas pontas o contam
    /// ([ADR-0010](../../../docs/adr/0010-canal-de-dados-em-tcp-proprio.md)).
    ///
    /// # Errors
    ///
    /// [`CryptoError::Open`] se o Noise recusar a cifragem — em especial quando o texto claro
    /// passa de 65 519 B, que é o teto de uma mensagem Noise menos a tag.
    pub fn seal(&mut self, plaintext: &[u8]) -> Result<(u64, Vec<u8>)> {
        let counter = self.counter.wrapping_add(1);
        self.counter = counter;
        let mut out = vec![0u8; plaintext.len() + TAG];
        let len = self
            .state
            .write_message(counter, plaintext, &mut out)
            .map_err(|_| CryptoError::Open)?;
        out.truncate(len);
        Ok((counter, out))
    }

    /// Se já convém rechavear, por volume de mensagens enviadas.
    #[must_use]
    pub const fn should_rekey(&self) -> bool {
        self.counter >= REKEY_AFTER
    }
}

/// A metade que decifra: o estado do Noise e a janela de repetição.
pub struct Opener {
    state: Arc<StatelessTransportState>,
    replay: ReplayWindow,
}

impl core::fmt::Debug for Opener {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Opener").finish_non_exhaustive()
    }
}

impl Opener {
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
}

/// O transporte cifrado de uma sessão: as duas metades juntas.
///
/// Quem usa um portador de entrada trata o transporte como uma coisa só, e é assim que os três
/// portadores sempre o usaram. Quem precisa cifrar e decifrar **ao mesmo tempo**, em tarefas
/// diferentes, usa [`Transport::split`].
pub struct Transport {
    sealer: Sealer,
    opener: Opener,
}

impl core::fmt::Debug for Transport {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Transport")
            .field("send_counter", &self.sealer.counter)
            .finish_non_exhaustive()
    }
}

impl Transport {
    pub(crate) fn new(state: StatelessTransportState) -> Self {
        let state = Arc::new(state);
        Self {
            sealer: Sealer {
                state: Arc::clone(&state),
                counter: 0,
            },
            opener: Opener {
                state,
                replay: ReplayWindow::new(),
            },
        }
    }

    /// Cifra um quadro. Ver [`Sealer::seal`].
    ///
    /// # Errors
    ///
    /// Os de [`Sealer::seal`].
    pub fn seal(&mut self, plaintext: &[u8]) -> Result<(u64, Vec<u8>)> {
        self.sealer.seal(plaintext)
    }

    /// Decifra um quadro. Ver [`Opener::open`].
    ///
    /// # Errors
    ///
    /// Os de [`Opener::open`].
    pub fn open(&mut self, counter: u64, ciphertext: &[u8]) -> Result<Vec<u8>> {
        self.opener.open(counter, ciphertext)
    }

    /// Se já convém rechavear, por volume de mensagens enviadas.
    #[must_use]
    pub const fn should_rekey(&self) -> bool {
        self.sealer.should_rekey()
    }

    /// Separa as duas metades, para cifrar e decifrar em tarefas diferentes.
    ///
    /// # Por que isto é seguro
    ///
    /// O estado de transporte do Noise, na forma *stateless*, é **imutável em uso**:
    /// `write_message` e `read_message` recebem `&self` e o nonce vem de fora. As chaves não
    /// mudam a cada mensagem — o que muda é o contador, e ele é do chamador.
    ///
    /// Logo o único estado mutável são duas coisas que **não se cruzam**: o contador de envio,
    /// que só o [`Sealer`] toca, e a janela de repetição, que só o [`Opener`] toca. Dividir não
    /// cria disputa; ele só reconhece uma separação que já existia.
    ///
    /// Sem isto, um canal cheio-duplex precisaria de um `Mutex` no caminho de cada bloco de
    /// arquivo — trava e contenção para proteger um estado que nunca é escrito.
    #[must_use]
    pub fn split(self) -> (Sealer, Opener) {
        (self.sealer, self.opener)
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
    fn the_two_halves_work_apart_exactly_as_the_whole_did() {
        // A propriedade que `split` promete: separar não muda o que sai no fio nem o que é
        // aceito do outro lado.
        let (a, b) = paired();
        let (mut sealer, _) = a.split();
        let (_, mut opener) = b.split();
        for n in 0u8..16 {
            let (counter, ciphertext) = sealer.seal(&[n; 300]).unwrap();
            assert_eq!(opener.open(counter, &ciphertext).unwrap(), vec![n; 300]);
        }
    }

    #[test]
    fn a_split_opener_still_refuses_a_replay() {
        // A janela de repetição foi para o lado certo da divisão.
        let (mut a, b) = paired();
        let (_, mut opener) = b.split();
        let (counter, sealed) = a.seal(b"tecla").unwrap();
        assert!(opener.open(counter, &sealed).is_ok());
        assert_eq!(
            opener.open(counter, &sealed).unwrap_err(),
            crate::error::CryptoError::Replay
        );
    }

    #[test]
    fn a_split_sealer_keeps_counting_from_where_it_was() {
        // O contador não volta a zero na divisão: um transporte que já enviou dois quadros
        // continua no três. Reiniciar seria reusar nonce, que é a falha mais grave possível aqui.
        let (mut a, b) = paired();
        let _ = a.seal(b"um").unwrap();
        let (counter_antes, _) = a.seal(b"dois").unwrap();
        let (mut sealer, _) = a.split();
        let (counter_depois, ciphertext) = sealer.seal(b"tres").unwrap();
        assert_eq!(counter_antes, 2);
        assert_eq!(counter_depois, 3, "o contador não pode reiniciar");

        let (_, mut opener) = b.split();
        assert_eq!(opener.open(counter_depois, &ciphertext).unwrap(), b"tres");
    }

    #[test]
    fn the_plaintext_ceiling_of_a_noise_message_is_where_we_think_it_is() {
        // O número que o ADR-0010 corrigiu, medido contra o `snow` e não contra a especificação
        // lida por cima: 65 519 B passam, um byte mais não.
        let (mut a, mut b) = paired();
        let teto = ir_proto::limits::MAX_TCP_PLAINTEXT;
        let (counter, sealed) = a.seal(&vec![0u8; teto]).expect("o teto tem de caber");
        assert_eq!(sealed.len(), teto + 16);
        assert_eq!(b.open(counter, &sealed).unwrap().len(), teto);

        assert!(
            a.seal(&vec![0u8; teto + 1]).is_err(),
            "um byte acima do teto do Noise não pode ser cifrado"
        );
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
