//! O *stream* de bytes, e a leitura por quadros em cima dele.
//!
//! Acima deste módulo — handshake, enlace cifrado, transferência — tudo trabalha contra
//! [`Channel`], que é só "um fluxo de bytes nos dois sentidos". Abaixo dele fica o `TcpStream`.
//!
//! O efeito prático é que o canal de dados inteiro é testável com [`tokio::io::duplex`], sem
//! duas máquinas e sem rede. É a mesma escolha do `ir-bt`, e pelo mesmo motivo: foi a
//! impossibilidade de testar transporte sem hardware que deixou partes do v1 sem um único teste
//! ([00, §6](../../../docs/00-licoes-do-v1.md)).
//!
//! # Junto para o handshake, separado para a transferência
//!
//! O handshake é passo a passo: escreve, lê, escreve. Ele quer o *stream* inteiro numa mão só, e
//! é o que [`Frames`] dá.
//!
//! A transferência é o oposto: um lado despeja blocos enquanto o outro devolve confirmações, e as
//! duas coisas acontecem ao mesmo tempo. Por isso [`Frames::split`] existe.
//!
//! **A alternativa seria um `select!` sobre um enlace único, e ela está errada.** [`Frames::recv`]
//! lê do socket para um buffer e só depois passa os bytes ao desenquadrador — duas etapas. Se o
//! futuro for descartado entre elas, o que o socket já entregou desaparece: o TCP considera
//! aqueles bytes entregues e ninguém os pede de novo. O sintoma seria um bloco de arquivo
//! faltando, no meio de uma transferência de gigabytes, sem erro em lugar nenhum. Dividir em duas
//! tarefas remove a categoria do problema em vez de tentar acertar o cancelamento.

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, ReadHalf, WriteHalf, split};

use crate::bulk::wire::{self, Framer, MAX_BODY, PREFIX};
use crate::error::{NetError, Result};

/// Um fluxo de bytes bidirecional com o par.
///
/// Não há nada a implementar: quem já é `AsyncRead + AsyncWrite` serve. O *trait* existe para dar
/// nome à exigência e para o resto do módulo não repetir quatro limites em cada assinatura.
pub trait Channel: AsyncRead + AsyncWrite + Unpin + Send {}

impl<T: AsyncRead + AsyncWrite + Unpin + Send> Channel for T {}

/// Quanto se lê do *stream* de uma vez.
///
/// Um quadro inteiro com folga. Ler de mais não custa: o que sobrar fica no desenquadrador e vira
/// o quadro seguinte.
const READ_CHUNK: usize = MAX_BODY + PREFIX;

/// A metade que escreve quadros.
#[derive(Debug)]
pub struct FrameWriter<W> {
    inner: W,
}

impl<W: AsyncWrite + Unpin + Send> FrameWriter<W> {
    /// Envolve a ponta de escrita.
    pub const fn new(inner: W) -> Self {
        Self { inner }
    }

    /// Manda um corpo, com o prefixo de tamanho na frente.
    ///
    /// Sem `flush`: num canal que despeja blocos de 60 KiB, forçar a saída a cada bloco impede o
    /// TCP de formar segmentos cheios, que é exatamente o que se quer que ele faça.
    ///
    /// # Errors
    ///
    /// [`NetError::TooLarge`] se o corpo passa do teto do portador; [`NetError::Io`] se o socket
    /// falhar.
    pub async fn send(&mut self, body: &[u8]) -> Result<()> {
        let framed = wire::frame(body)?;
        self.inner.write_all(&framed).await?;
        Ok(())
    }

    /// Manda um corpo e força a saída, para quando a resposta do par é o próximo passo.
    ///
    /// No handshake, ficar num buffer intermediário é impasse e não atraso: o outro lado espera
    /// uma mensagem que já foi escrita.
    ///
    /// # Errors
    ///
    /// Os mesmos de [`Self::send`].
    pub async fn send_now(&mut self, body: &[u8]) -> Result<()> {
        self.send(body).await?;
        self.inner.flush().await?;
        Ok(())
    }
}

/// A metade que lê quadros.
#[derive(Debug)]
pub struct FrameReader<R> {
    inner: R,
    framer: Framer,
    buffer: Box<[u8]>,
}

impl<R: AsyncRead + Unpin + Send> FrameReader<R> {
    /// Envolve a ponta de leitura, com o desenquadrador que já vem do handshake.
    ///
    /// O desenquadrador é recebido, e não criado, porque ele pode **já ter bytes**: o par tem o
    /// direito de emendar o primeiro quadro de dados no mesmo segmento da última mensagem do
    /// handshake. Descartá-lo perderia esse quadro.
    pub fn with_framer(inner: R, framer: Framer) -> Self {
        Self {
            inner,
            framer,
            buffer: vec![0u8; READ_CHUNK].into_boxed_slice(),
        }
    }

    /// Espera o próximo corpo completo.
    ///
    /// # Errors
    ///
    /// [`NetError::Closed`] se o par fechou o *stream*; [`NetError::TooLarge`] se ele anunciou um
    /// tamanho absurdo; [`NetError::Io`] em falha de socket.
    pub async fn recv(&mut self) -> Result<Vec<u8>> {
        loop {
            if let Some(body) = self.framer.next_body()? {
                return Ok(body);
            }
            let read = self.inner.read(&mut self.buffer).await?;
            let Some(arrived) = self.buffer.get(..read).filter(|_| read > 0) else {
                // Leitura de zero byte é fim de fluxo, não falha de socket. A distinção é o que
                // permite dizer "o par encerrou" em vez de "erro de E/S".
                return Err(NetError::Closed);
            };
            self.framer.feed(arrived);
        }
    }
}

/// Um *stream* visto como uma sequência de quadros, nos dois sentidos.
///
/// A forma usada no handshake. Depois dele, [`Self::split`].
#[derive(Debug)]
pub struct Frames<C> {
    channel: C,
    framer: Framer,
    buffer: Box<[u8]>,
}

impl<C: Channel> Frames<C> {
    /// Envolve um *stream*.
    #[must_use]
    pub fn new(channel: C) -> Self {
        Self {
            channel,
            framer: Framer::new(),
            buffer: vec![0u8; READ_CHUNK].into_boxed_slice(),
        }
    }

    /// Manda um corpo. Ver [`FrameWriter::send`].
    ///
    /// # Errors
    ///
    /// Os de [`FrameWriter::send`].
    pub async fn send(&mut self, body: &[u8]) -> Result<()> {
        let framed = wire::frame(body)?;
        self.channel.write_all(&framed).await?;
        Ok(())
    }

    /// Manda um corpo e força a saída. Ver [`FrameWriter::send_now`].
    ///
    /// # Errors
    ///
    /// Os de [`FrameWriter::send`].
    pub async fn send_now(&mut self, body: &[u8]) -> Result<()> {
        self.send(body).await?;
        self.channel.flush().await?;
        Ok(())
    }

    /// Espera o próximo corpo completo. Ver [`FrameReader::recv`].
    ///
    /// # Errors
    ///
    /// Os de [`FrameReader::recv`].
    pub async fn recv(&mut self) -> Result<Vec<u8>> {
        loop {
            if let Some(body) = self.framer.next_body()? {
                return Ok(body);
            }
            let read = self.channel.read(&mut self.buffer).await?;
            let Some(arrived) = self.buffer.get(..read).filter(|_| read > 0) else {
                return Err(NetError::Closed);
            };
            self.framer.feed(arrived);
        }
    }

    /// Separa em duas metades independentes, preservando os bytes já recebidos.
    #[must_use]
    pub fn split(self) -> (FrameReader<ReadHalf<C>>, FrameWriter<WriteHalf<C>>) {
        let (read, write) = split(self.channel);
        (
            FrameReader::with_framer(read, self.framer),
            FrameWriter::new(write),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pair() -> (
        Frames<tokio::io::DuplexStream>,
        Frames<tokio::io::DuplexStream>,
    ) {
        let (a, b) = tokio::io::duplex(READ_CHUNK * 4);
        (Frames::new(a), Frames::new(b))
    }

    #[tokio::test]
    async fn a_body_sent_arrives_whole() {
        let (mut here, mut there) = pair();
        here.send_now(b"manifesto").await.unwrap();
        assert_eq!(there.recv().await.unwrap(), b"manifesto");
    }

    #[tokio::test]
    async fn bodies_keep_their_order_and_their_boundaries() {
        let (mut here, mut there) = pair();
        for n in 0u8..8 {
            here.send(&vec![n; 1000]).await.unwrap();
        }
        for n in 0u8..8 {
            assert_eq!(there.recv().await.unwrap(), vec![n; 1000]);
        }
    }

    #[tokio::test]
    async fn the_largest_possible_body_survives_the_round_trip() {
        // O caso que o limite corrigido do ADR-0010 tornou possível — e que com o número antigo
        // nem teria sido cifrável.
        let (mut here, mut there) = pair();
        let body = vec![0xa5; MAX_BODY];
        tokio::spawn(async move { here.send_now(&body).await });
        assert_eq!(there.recv().await.unwrap().len(), MAX_BODY);
    }

    #[tokio::test]
    async fn a_closed_stream_reads_as_the_peer_leaving() {
        let (here, mut there) = pair();
        drop(here);
        assert!(matches!(there.recv().await, Err(NetError::Closed)));
    }

    #[tokio::test]
    async fn a_lying_prefix_on_the_wire_fails_the_link_instead_of_allocating() {
        // Um par hostil escreve o prefixo à mão, sem passar por `frame`.
        let (mut raw, there) = tokio::io::duplex(64);
        tokio::spawn(async move {
            let _ = raw.write_all(&u32::MAX.to_le_bytes()).await;
            let _ = raw.flush().await;
            // Segura o socket aberto: o erro tem de vir do prefixo, não do fim do fluxo.
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        });
        let mut frames = Frames::new(there);
        assert!(matches!(
            frames.recv().await,
            Err(NetError::TooLarge { .. })
        ));
    }

    #[tokio::test]
    async fn splitting_keeps_bytes_that_arrived_before_the_split() {
        // O caso que `with_framer` existe para não perder: o par emenda o quadro seguinte no
        // mesmo segmento, e a divisão acontece depois de ele já estar no desenquadrador.
        let (mut here, mut there) = pair();
        here.send_now(b"primeiro").await.unwrap();
        here.send_now(b"segundo").await.unwrap();

        // Lê só o primeiro: o segundo fica pendente dentro do desenquadrador.
        assert_eq!(there.recv().await.unwrap(), b"primeiro");
        let (mut reader, _writer) = there.split();
        assert_eq!(reader.recv().await.unwrap(), b"segundo");
    }

    #[tokio::test]
    async fn the_two_halves_work_at_the_same_time() {
        // A propriedade que justifica a divisão: escrever e ler em tarefas diferentes, sem
        // `select!` e sem risco de cancelamento no meio de uma leitura.
        let (here, there) = pair();
        let (mut nosso_leitor, mut nosso_escritor) = here.split();
        let (mut leitor_do_par, mut escritor_do_par) = there.split();

        let despeja = tokio::spawn(async move {
            for n in 0u8..32 {
                nosso_escritor.send(&vec![n; 500]).await.unwrap();
            }
        });
        let confirma = tokio::spawn(async move {
            for n in 0u8..32 {
                assert_eq!(leitor_do_par.recv().await.unwrap(), vec![n; 500]);
                escritor_do_par.send_now(&[n]).await.unwrap();
            }
        });
        for n in 0u8..32 {
            assert_eq!(nosso_leitor.recv().await.unwrap(), vec![n]);
        }
        despeja.await.unwrap();
        confirma.await.unwrap();
    }
}
