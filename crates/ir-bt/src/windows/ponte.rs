//! A ponte entre o socket síncrono do Winsock e o mundo assíncrono do `tokio`.
//!
//! O `AF_BTH` é uma API bloqueante e não se registra no reator do `tokio`. A ponte põe o socket
//! em duas threads — uma que lê, outra que escreve — e as liga ao runtime por canais. O que sai
//! daqui implementa `AsyncRead` e `AsyncWrite`, e portanto serve como [`Canal`](crate::Canal)
//! igual ao `Stream` do Linux.
//!
//! Este módulo é **seguro**: todo o `unsafe` fica em [`winsock`](super::winsock), e aqui só se
//! usam os embrulhos.
//!
//! # Fechar sem travar
//!
//! Uma thread parada num `recv` não sai sozinha: ela espera bytes que talvez nunca venham. Por
//! isso o fechamento chama `shutdown` **antes** de fechar o socket — é ele que faz o `recv`
//! voltar na hora e as threads terminarem. Sem esse detalhe, encerrar o serviço ficaria
//! esperando o par falar.

use std::io;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::sync::mpsc;

use super::winsock::{self, Sock};

/// Quanto se lê do socket por vez.
const LEITURA: usize = 4096;

/// O dono do socket: fecha uma vez só, quando ninguém mais o usa.
#[derive(Debug)]
struct Dono(Sock);

impl Drop for Dono {
    fn drop(&mut self) {
        winsock::fechar(self.0);
    }
}

/// Um socket RFCOMM do Windows, vestido de fluxo assíncrono.
#[derive(Debug)]
pub struct CanalDeSocket {
    entrada: mpsc::UnboundedReceiver<io::Result<Vec<u8>>>,
    saida: Option<mpsc::UnboundedSender<Vec<u8>>>,
    /// O que sobrou de uma leitura que não coube no buffer de quem pediu.
    sobra: Vec<u8>,
    lido_da_sobra: usize,
    dono: Arc<Dono>,
}

impl CanalDeSocket {
    /// Põe o socket para trabalhar em duas threads e devolve o fluxo.
    pub(super) fn novo(sock: Sock) -> Self {
        let dono = Arc::new(Dono(sock));
        let (entrada_tx, entrada) = mpsc::unbounded_channel();
        let (saida, saida_rx) = mpsc::unbounded_channel();

        girar_leitura(sock, Arc::clone(&dono), entrada_tx);
        girar_escrita(sock, Arc::clone(&dono), saida_rx);

        Self {
            entrada,
            saida: Some(saida),
            sobra: Vec::new(),
            lido_da_sobra: 0,
            dono,
        }
    }

    /// Copia da sobra para o buffo de quem pediu. Devolve se copiou alguma coisa.
    fn servir_da_sobra(&mut self, buffer: &mut ReadBuf<'_>) -> bool {
        let Some(restante) = self.sobra.get(self.lido_da_sobra..) else {
            return false;
        };
        if restante.is_empty() {
            return false;
        }
        let quanto = restante.len().min(buffer.remaining());
        let Some(pedaco) = restante.get(..quanto) else {
            return false;
        };
        buffer.put_slice(pedaco);
        self.lido_da_sobra += quanto;
        true
    }
}

impl Drop for CanalDeSocket {
    fn drop(&mut self) {
        // Solta a ponta de escrita, para a thread de escrita terminar, e desbloqueia a leitura.
        self.saida = None;
        winsock::encerrar(self.dono.0);
    }
}

/// A thread que lê do socket e empurra para o runtime.
fn girar_leitura(
    sock: Sock,
    dono: Arc<Dono>,
    para_o_runtime: mpsc::UnboundedSender<io::Result<Vec<u8>>>,
) {
    std::thread::spawn(move || {
        let _dono = dono; // mantém o socket aberto enquanto esta thread o usa
        let mut buffer = vec![0u8; LEITURA];
        loop {
            match winsock::receber(sock, &mut buffer) {
                Ok(0) => break, // o par fechou
                Ok(lidos) => {
                    let pedaco = buffer.get(..lidos).unwrap_or(&[]).to_vec();
                    if para_o_runtime.send(Ok(pedaco)).is_err() {
                        break; // ninguém mais escuta
                    }
                }
                Err(erro) => {
                    let _ = para_o_runtime.send(Err(erro));
                    break;
                }
            }
        }
    });
}

/// A thread que recebe do runtime e escreve no socket.
fn girar_escrita(sock: Sock, dono: Arc<Dono>, mut do_runtime: mpsc::UnboundedReceiver<Vec<u8>>) {
    std::thread::spawn(move || {
        let _dono = dono;
        while let Some(bytes) = do_runtime.blocking_recv() {
            if winsock::enviar_tudo(sock, &bytes).is_err() {
                // A leitura vai perceber a mesma falha e contá-la a quem espera; repetir o erro
                // aqui não acrescenta nada.
                break;
            }
        }
    });
}

impl AsyncRead for CanalDeSocket {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buffer: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        if self.servir_da_sobra(buffer) {
            return Poll::Ready(Ok(()));
        }
        match self.entrada.poll_recv(cx) {
            Poll::Ready(Some(Ok(pedaco))) => {
                self.sobra = pedaco;
                self.lido_da_sobra = 0;
                self.servir_da_sobra(buffer);
                Poll::Ready(Ok(()))
            }
            Poll::Ready(Some(Err(erro))) => Poll::Ready(Err(erro)),
            // Canal fechado é fim de fluxo: zero byte lido, que é como se diz "acabou".
            Poll::Ready(None) => Poll::Ready(Ok(())),
            Poll::Pending => Poll::Pending,
        }
    }
}

impl AsyncWrite for CanalDeSocket {
    fn poll_write(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
        bytes: &[u8],
    ) -> Poll<io::Result<usize>> {
        let Some(saida) = self.saida.as_ref() else {
            return Poll::Ready(Err(io::Error::from(io::ErrorKind::BrokenPipe)));
        };
        match saida.send(bytes.to_vec()) {
            Ok(()) => Poll::Ready(Ok(bytes.len())),
            Err(_) => Poll::Ready(Err(io::Error::from(io::ErrorKind::BrokenPipe))),
        }
    }

    /// Não há o que esvaziar aqui.
    ///
    /// A fila para a thread de escrita é ilimitada, então escrever nunca fica pendente, e a
    /// thread despacha o que houver na ordem. O que `flush` garante — "os bytes estão a
    /// caminho, na ordem" — já vale no instante em que `poll_write` devolve.
    fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        self.saida = None;
        winsock::encerrar(self.dono.0);
        Poll::Ready(Ok(()))
    }
}
