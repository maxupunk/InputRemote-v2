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
//! # Escrever sem acumular
//!
//! A ponte deixa no máximo [`EM_VOO`] quadros esperando a thread de escrita. Sem teto, um rádio
//! travado por interferência acumulava aqui, **depois** da cifragem, segundos de quadros que não
//! podem mais ser descartados — o contador do enlace é implícito, e pular um quadro cifrado
//! derrubaria o enlace. Com o teto, a espera volta para o endpoint, que descarta o que ficou velho
//! **antes** de cifrar ([`VELHO_DEMAIS`](crate::endpoint::VELHO_DEMAIS)).
//!
//! # Fechar sem travar
//!
//! Uma thread parada num `recv` não sai sozinha: ela espera bytes que talvez nunca venham. Por
//! isso o fechamento chama `shutdown` **antes** de fechar o socket — é ele que faz o `recv`
//! voltar na hora e as threads terminarem. Sem esse detalhe, encerrar o serviço ficaria
//! esperando o par falar.

use std::io;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Waker};

use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::sync::mpsc;

use super::winsock::{self, Sock};

/// Quanto se lê do socket por vez.
const LEITURA: usize = 4096;

/// Quantos quadros podem esperar a thread de escrita.
///
/// Poucos: o bastante para a thread nunca ficar parada entre dois quadros enquanto o rádio escoa,
/// e pouco o bastante para, com o rádio travado, a fila ficar antes da cifragem e não aqui.
const EM_VOO: usize = 4;

/// O controle de vazão entre quem escreve e a thread de escrita.
#[derive(Debug, Default)]
struct Vazao {
    /// Quantos quadros foram entregues à thread e ainda não saíram pelo socket.
    em_voo: AtomicUsize,
    /// A thread de escrita acabou: nada mais sai por aqui.
    fechada: AtomicBool,
    /// Quem está esperando vaga.
    esperando: Mutex<Option<Waker>>,
}

impl Vazao {
    /// Guarda quem espera vaga, para a thread acordá-lo.
    fn esperar(&self, cx: &Context<'_>) {
        if let Ok(mut esperando) = self.esperando.lock() {
            *esperando = Some(cx.waker().clone());
        }
    }

    /// Acorda quem esperava vaga — ou o fim.
    fn acordar(&self) {
        let quem = self
            .esperando
            .lock()
            .ok()
            .and_then(|mut esperando| esperando.take());
        if let Some(waker) = quem {
            waker.wake();
        }
    }

    /// Um quadro saiu pelo socket.
    fn saiu(&self) {
        self.em_voo.fetch_sub(1, Ordering::AcqRel);
        self.acordar();
    }

    /// A thread de escrita acabou.
    fn fechar(&self) {
        self.fechada.store(true, Ordering::Release);
        self.acordar();
    }

    /// Se há vaga para mais um quadro.
    fn ha_vaga(&self) -> bool {
        self.em_voo.load(Ordering::Acquire) < EM_VOO
    }
}

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
    /// Quantos quadros a thread de escrita ainda tem por mandar ([`EM_VOO`]).
    vazao: Arc<Vazao>,
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

        let vazao = Arc::new(Vazao::default());
        girar_leitura(sock, Arc::clone(&dono), entrada_tx);
        girar_escrita(sock, Arc::clone(&dono), saida_rx, Arc::clone(&vazao));

        Self {
            entrada,
            saida: Some(saida),
            vazao,
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
fn girar_escrita(
    sock: Sock,
    dono: Arc<Dono>,
    mut do_runtime: mpsc::UnboundedReceiver<Vec<u8>>,
    vazao: Arc<Vazao>,
) {
    std::thread::spawn(move || {
        let _dono = dono;
        while let Some(bytes) = do_runtime.blocking_recv() {
            let enviado = winsock::enviar_tudo(sock, &bytes);
            vazao.saiu();
            if enviado.is_err() {
                // A leitura vai perceber a mesma falha e contá-la a quem espera; repetir o erro
                // aqui não acrescenta nada.
                break;
            }
        }
        // Quem espera vaga precisa saber que ela não vem mais — senão fica parado para sempre
        // num `write` e nunca volta a ler o erro que derrubaria o enlace.
        vazao.fechar();
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
        cx: &mut Context<'_>,
        bytes: &[u8],
    ) -> Poll<io::Result<usize>> {
        let quebrado = || Poll::Ready(Err(io::Error::from(io::ErrorKind::BrokenPipe)));
        let Some(saida) = self.saida.as_ref() else {
            return quebrado();
        };
        if self.vazao.fechada.load(Ordering::Acquire) {
            return quebrado();
        }
        if !self.vazao.ha_vaga() {
            self.vazao.esperar(cx);
            // A vaga pode ter aberto entre a conferência e o registro de quem espera; sem esta
            // segunda olhada, o despertar se perderia e a escrita ficaria parada para sempre.
            if !self.vazao.ha_vaga() && !self.vazao.fechada.load(Ordering::Acquire) {
                return Poll::Pending;
            }
        }
        self.vazao.em_voo.fetch_add(1, Ordering::AcqRel);
        match saida.send(bytes.to_vec()) {
            Ok(()) => Poll::Ready(Ok(bytes.len())),
            Err(_) => quebrado(),
        }
    }

    /// Não há o que esvaziar aqui.
    ///
    /// Quem entrou na fila da thread de escrita já está a caminho, na ordem; o teto de
    /// [`EM_VOO`] é cobrado em `poll_write`, antes de entrar. O que `flush` garante — "os bytes
    /// estão a caminho, na ordem" — já vale no instante em que `poll_write` devolve.
    fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        self.saida = None;
        winsock::encerrar(self.dono.0);
        Poll::Ready(Ok(()))
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::task::{Context, Wake, Waker};

    use super::{EM_VOO, Vazao};

    /// Um despertador que só conta quantas vezes foi chamado.
    #[derive(Default)]
    struct Contador(AtomicUsize);

    impl Wake for Contador {
        fn wake(self: Arc<Self>) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }

    fn cheia() -> Vazao {
        let vazao = Vazao::default();
        vazao.em_voo.store(EM_VOO, Ordering::SeqCst);
        vazao
    }

    #[test]
    fn com_o_teto_atingido_nao_ha_vaga() {
        let vazao = cheia();
        assert!(!vazao.ha_vaga());
        vazao.em_voo.store(EM_VOO - 1, Ordering::SeqCst);
        assert!(vazao.ha_vaga());
    }

    #[test]
    fn o_quadro_que_sai_abre_vaga_e_acorda_quem_esperava() {
        let vazao = cheia();
        let contador = Arc::new(Contador::default());
        let waker = Waker::from(Arc::clone(&contador));
        vazao.esperar(&Context::from_waker(&waker));

        vazao.saiu();

        assert!(vazao.ha_vaga());
        assert_eq!(contador.0.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn a_thread_que_acaba_acorda_quem_esperava_para_ele_ver_o_fim() {
        // Sem isto a escrita ficaria parada para sempre, e o endpoint nunca voltaria a ler o erro
        // que derrubaria o enlace.
        let vazao = cheia();
        let contador = Arc::new(Contador::default());
        let waker = Waker::from(Arc::clone(&contador));
        vazao.esperar(&Context::from_waker(&waker));

        vazao.fechar();

        assert!(vazao.fechada.load(Ordering::SeqCst));
        assert_eq!(contador.0.load(Ordering::SeqCst), 1);
    }
}
