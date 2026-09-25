//! O enlace cifrado do canal de dados: quadro em claro de um lado, corpo cifrado do outro.
//!
//! Faz o que o [`SecureLink`](crate::link::SecureLink) do UDP faz, com **uma** diferença, que é a
//! mesma do `ir-bt`: o contador do Noise não vai no fio. Ele é contado nas duas pontas.
//!
//! O contador recebido só avança **depois** de a tag conferir. Se avançasse antes, um corpo
//! forjado queimaria aquele número e o quadro legítimo seguinte — que virá com ele — deixaria de
//! abrir. Um atacante que só consegue escrever lixo no socket não deve conseguir dessincronizar
//! um enlace que, sem ele, funcionaria. A regra mora no [`ContadorImplicito`], o mesmo do rádio.

use ir_crypto::enlace::ContadorImplicito;
use ir_crypto::{Opener, Sealer, Transport};
use tokio::io::{AsyncRead, AsyncWrite, ReadHalf, WriteHalf};

use crate::bulk::stream::{Channel, FrameReader, FrameWriter, Frames};
use crate::error::Result;

/// Um enlace cifrado sobre um *stream*, com contador implícito nas duas direções.
///
/// A forma de uma mão só, usada no handshake e em teste. A transferência usa
/// [`Self::split`]. Por dentro já são as duas metades, e cada método só as chama.
#[derive(Debug)]
pub struct BulkLink<C> {
    receiver: BulkReceiver<ReadHalf<C>>,
    sender: BulkSender<WriteHalf<C>>,
}

impl<C: Channel> BulkLink<C> {
    /// Monta o enlace sobre um *stream* que já passou pelo handshake.
    #[must_use]
    pub fn new(frames: Frames<C>, transport: Transport) -> Self {
        let (reader, writer) = frames.split();
        let (sealer, opener) = transport.split();
        Self {
            receiver: BulkReceiver {
                reader,
                opener,
                contagem: ContadorImplicito::novo(),
            },
            sender: BulkSender { writer, sealer },
        }
    }

    /// Cifra e manda um quadro, sem forçar a saída. Ver [`BulkSender::send`].
    ///
    /// # Errors
    ///
    /// [`NetError::Crypto`](crate::NetError::Crypto) se o Noise recusar;
    /// [`NetError::Io`](crate::NetError::Io) em falha de socket.
    pub async fn send(&mut self, plaintext: &[u8]) -> Result<()> {
        self.sender.send(plaintext).await
    }

    /// Cifra, manda e força a saída. Ver [`BulkSender::send_now`].
    ///
    /// # Errors
    ///
    /// Os mesmos de [`Self::send`].
    pub async fn send_now(&mut self, plaintext: &[u8]) -> Result<()> {
        self.sender.send_now(plaintext).await
    }

    /// Espera o próximo quadro e o decifra. Ver [`BulkReceiver::recv`].
    ///
    /// # Errors
    ///
    /// [`NetError::Closed`](crate::NetError::Closed) se o par encerrou;
    /// [`NetError::Crypto`](crate::NetError::Crypto) se a tag não conferir — e aí o enlace
    /// **precisa** cair, porque a contagem das duas pontas divergiu.
    pub async fn recv(&mut self) -> Result<Vec<u8>> {
        self.receiver.recv().await
    }

    /// Separa o enlace em duas metades independentes, para cifrar e decifrar ao mesmo tempo.
    ///
    /// Nenhuma trava e nenhum estado compartilhado mutável: ver [`Transport::split`].
    #[must_use]
    pub fn split(self) -> (BulkReceiver<ReadHalf<C>>, BulkSender<WriteHalf<C>>) {
        (self.receiver, self.sender)
    }
}

/// A metade que cifra e escreve.
#[derive(Debug)]
pub struct BulkSender<W> {
    writer: FrameWriter<W>,
    sealer: Sealer,
}

impl<W: AsyncWrite + Unpin + Send> BulkSender<W> {
    /// Cifra e manda um quadro, deixando o TCP juntar segmentos.
    ///
    /// # Errors
    ///
    /// Como [`BulkLink::send`].
    pub async fn send(&mut self, plaintext: &[u8]) -> Result<()> {
        let (_, ciphertext) = self.sealer.seal(plaintext)?;
        self.writer.send(&ciphertext).await
    }

    /// Cifra, manda e força a saída — para quando o par está esperando por isto.
    ///
    /// # Errors
    ///
    /// Como [`BulkLink::send`].
    pub async fn send_now(&mut self, plaintext: &[u8]) -> Result<()> {
        let (_, ciphertext) = self.sealer.seal(plaintext)?;
        self.writer.send_now(&ciphertext).await
    }

    /// Se já convém rechavear, por volume enviado.
    #[must_use]
    pub const fn should_rekey(&self) -> bool {
        self.sealer.should_rekey()
    }
}

/// A metade que lê e decifra.
#[derive(Debug)]
pub struct BulkReceiver<R> {
    reader: FrameReader<R>,
    opener: Opener,
    contagem: ContadorImplicito,
}

impl<R: AsyncRead + Unpin + Send> BulkReceiver<R> {
    /// Espera o próximo quadro e o decifra.
    ///
    /// # Errors
    ///
    /// Como [`BulkLink::recv`].
    pub async fn recv(&mut self) -> Result<Vec<u8>> {
        let ciphertext = self.reader.recv().await?;
        let opener = &mut self.opener;
        Ok(self
            .contagem
            .abrir(|counter| opener.open(counter, &ciphertext))?)
    }
}

#[cfg(test)]
mod tests {
    use ir_crypto::{Handshake, Identity};
    use tokio::io::{AsyncWriteExt, DuplexStream};

    use super::*;
    use crate::bulk::wire;
    use crate::error::NetError;

    /// Conduz um `IK` inteiro em memória e devolve os dois transportes.
    fn handshaken() -> (Transport, Transport) {
        let here = Identity::generate();
        let there = Identity::generate();
        let mut initiator =
            Handshake::reconnect_initiator(&here, there.public()).expect("iniciador");
        let mut responder = Handshake::reconnect_responder(&there).expect("respondedor");
        // `IK` tem duas mensagens: iniciador → respondedor → iniciador.
        let first = initiator.write_message().expect("primeira");
        responder.read_message(&first).expect("lê primeira");
        let second = responder.write_message().expect("segunda");
        initiator.read_message(&second).expect("lê segunda");
        (
            initiator.into_transport().expect("transporte"),
            responder.into_transport().expect("transporte"),
        )
    }

    /// Dois enlaces cifrados ligados um no outro.
    fn linked() -> (BulkLink<DuplexStream>, BulkLink<DuplexStream>) {
        let (aqui, la) = handshaken();
        let (a, b) = tokio::io::duplex(wire::MAX_BODY * 4);
        (
            BulkLink::new(Frames::new(a), aqui),
            BulkLink::new(Frames::new(b), la),
        )
    }

    #[tokio::test]
    async fn a_frame_survives_the_round_trip() {
        let (mut here, mut there) = linked();
        here.send_now(b"manifesto").await.unwrap();
        assert_eq!(there.recv().await.unwrap(), b"manifesto");
    }

    #[tokio::test]
    async fn many_frames_stay_in_step_without_a_counter_on_the_wire() {
        // A prova de que a contagem implícita funciona: só está certo se as duas pontas
        // chegarem ao mesmo número sozinhas, quadro após quadro.
        let (mut here, mut there) = linked();
        for n in 0u8..64 {
            here.send(&[n; 200]).await.unwrap();
        }
        for n in 0u8..64 {
            assert_eq!(there.recv().await.unwrap(), vec![n; 200]);
        }
    }

    #[tokio::test]
    async fn the_counter_is_really_absent_from_the_wire() {
        // Confere o que o módulo promete: o corpo no fio é só o texto cifrado, sem os 8 bytes de
        // contador que o UDP carrega. Medido, não presumido.
        let (mut here, mut there) = linked();
        let plaintext = b"doze bytes..";
        here.send_now(plaintext).await.unwrap();
        let body = there.receiver.reader.recv().await.unwrap();
        assert_eq!(
            body.len(),
            plaintext.len() + 16,
            "o corpo deveria ser texto claro + tag, e nada mais"
        );
    }

    #[tokio::test]
    async fn the_split_halves_carry_the_counters_over() {
        // Dividir no meio de um enlace em uso não pode reiniciar contagem nenhuma: o emissor
        // continuaria de onde parou e o receptor esperaria do zero.
        let (mut here, mut there) = linked();
        here.send_now(b"antes").await.unwrap();
        assert_eq!(there.recv().await.unwrap(), b"antes");

        let (_, mut escritor) = here.split();
        let (mut leitor, _) = there.split();
        escritor.send_now(b"depois").await.unwrap();
        assert_eq!(leitor.recv().await.unwrap(), b"depois");
    }

    #[tokio::test]
    async fn blocks_flow_one_way_while_confirmations_flow_the_other() {
        // O padrão real do canal 5: um lado despeja blocos sem esperar, o outro confirma.
        let (here, there) = linked();
        let (mut nosso_leitor, mut nosso_escritor) = here.split();
        let (mut leitor_do_par, mut escritor_do_par) = there.split();

        let despeja = tokio::spawn(async move {
            for n in 0u8..48 {
                nosso_escritor.send(&vec![n; 4096]).await.unwrap();
            }
        });
        let confirma = tokio::spawn(async move {
            for n in 0u8..48 {
                assert_eq!(leitor_do_par.recv().await.unwrap(), vec![n; 4096]);
                escritor_do_par.send_now(&[n]).await.unwrap();
            }
        });
        for n in 0u8..48 {
            assert_eq!(nosso_leitor.recv().await.unwrap(), vec![n]);
        }
        despeja.await.unwrap();
        confirma.await.unwrap();
    }

    /// Uma bancada para escrever no fio à mão o que um par hostil escreveria: a ponta crua, o
    /// transporte que cifra de verdade, e o enlace que recebe.
    fn tamper_bench() -> (DuplexStream, Transport, BulkLink<DuplexStream>) {
        let (aqui, la) = handshaken();
        let (raw, mine) = tokio::io::duplex(wire::MAX_BODY * 4);
        (raw, aqui, BulkLink::new(Frames::new(mine), la))
    }

    #[tokio::test]
    async fn a_tampered_frame_is_refused() {
        let (mut raw, mut sender, mut receiver) = tamper_bench();
        let (_, mut ciphertext) = sender.seal(b"legitimo").unwrap();
        if let Some(byte) = ciphertext.first_mut() {
            *byte ^= 0x01;
        }
        raw.write_all(&wire::frame(&ciphertext).unwrap())
            .await
            .unwrap();
        raw.flush().await.unwrap();
        assert!(matches!(receiver.recv().await, Err(NetError::Crypto(_))));
    }

    #[tokio::test]
    async fn injected_garbage_does_not_burn_the_number_of_the_real_frame() {
        // É isto que a ordem "abrir, e só então avançar" compra. Se o contador avançasse na
        // tentativa, bastaria escrever lixo no socket para que o quadro legítimo seguinte — que
        // vem com aquele mesmo número — deixasse de abrir para sempre.
        //
        // A política de quem usa este enlace é derrubar no primeiro quadro que não abre, porque
        // sobre stream a causa provável é contagem dessincronizada. Mas a primitiva não deixa
        // sujeira própria, e é o que este teste fixa.
        let (mut raw, mut sender, mut receiver) = tamper_bench();
        raw.write_all(&wire::frame(&[0u8; 64]).unwrap())
            .await
            .unwrap();
        let (_, real) = sender.seal(b"legitimo").unwrap();
        raw.write_all(&wire::frame(&real).unwrap()).await.unwrap();
        raw.flush().await.unwrap();

        assert!(matches!(receiver.recv().await, Err(NetError::Crypto(_))));
        assert_eq!(
            receiver.receiver.contagem.recebidos(),
            0,
            "a falha não pode ter avançado nada"
        );
        assert_eq!(receiver.recv().await.unwrap(), b"legitimo");
        assert_eq!(receiver.receiver.contagem.recebidos(), 1);
    }
}
