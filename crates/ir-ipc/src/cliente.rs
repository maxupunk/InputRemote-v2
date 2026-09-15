//! Abrir o canal até o serviço, do lado de quem conecta: a janela e o agente.
//!
//! Os dois usam o canal do mesmo jeito — uma thread fica parada esperando o serviço falar, e outra
//! escreve quando tem algo a dizer — e por isso abrem o canal por aqui, num lugar só.
//!
//! # Por que o Windows não usa `std::fs::File`
//!
//! Um *named pipe* aberto pela biblioteca padrão vem **síncrono**, e o Windows serializa toda E/S
//! de um objeto de arquivo síncrono: enquanto uma thread está dentro de um `ReadFile` esperando o
//! serviço falar, a escrita de outra thread no mesmo objeto **espera a leitura terminar**. Um
//! `try_clone` não resolve, porque duplica o handle e mantém o mesmo objeto. O resultado é um
//! impasse que não aparece em teste com socket:
//!
//! - a janela abria, mandava o aperto de mão e travava — o serviço não tinha por que falar primeiro;
//! - "Parear" e "São iguais" só saíam quando, por acaso, chegava algum aviso do serviço;
//! - o agente só entregava o movimento do mouse quando o serviço mandava algum comando.
//!
//! Medido nesta máquina, com um servidor que lê o tempo todo e nunca responde (log 21):
//!
//! | Cliente | Escrita com leitura pendente |
//! |---|---|
//! | handle síncrono | bloqueada; o servidor recebeu 0 bytes |
//! | handle síncrono, sem leitura pendente | concluída |
//! | handle sobreposto | concluída |
//!
//! No Windows o canal é aberto com E/S sobreposta, pelo cliente de *named pipe* do `tokio` — o
//! mesmo mecanismo que o serviço já usa do lado dele. Quem chama continua vendo um [`Read`] e um
//! [`Write`] bloqueantes, como antes: o `tokio` fica escondido aqui, numa thread própria. No Linux
//! nada muda, porque leitura e escrita num socket Unix não se bloqueiam.

use std::io::{Read, Write};

/// As duas metades de um canal duplex: por onde se escreve e por onde se lê.
pub type Duplex = (Box<dyn Write + Send>, Box<dyn Read + Send>);

/// Abre um canal até o serviço, em `endereco`.
///
/// As duas metades podem ser usadas **ao mesmo tempo**, em threads diferentes — uma presa numa
/// leitura enquanto a outra escreve. É a garantia que este módulo existe para dar.
///
/// # Errors
///
/// O erro do sistema ao abrir. `NotFound` e afins significam que ninguém está escutando;
/// `PermissionDenied`, que o canal existe e este processo não o alcança.
pub fn abrir(endereco: &str) -> std::io::Result<Duplex> {
    plataforma::abrir(endereco)
}

#[cfg(not(windows))]
mod plataforma {
    use std::os::unix::net::UnixStream;

    use super::Duplex;

    pub(super) fn abrir(endereco: &str) -> std::io::Result<Duplex> {
        let escrita = UnixStream::connect(endereco)?;
        let leitura = escrita.try_clone()?;
        Ok((Box::new(escrita), Box::new(leitura)))
    }
}

#[cfg(windows)]
mod plataforma {
    use std::io::{Read, Write};
    use std::sync::Arc;

    use tokio::io::{AsyncReadExt, AsyncWriteExt, ReadHalf, WriteHalf};
    use tokio::net::windows::named_pipe::{ClientOptions, NamedPipeClient};
    use tokio::runtime::Runtime;

    use super::Duplex;

    pub(super) fn abrir(endereco: &str) -> std::io::Result<Duplex> {
        // Uma thread de trabalho basta: ela só acorda leitura e escrita quando o pipe fica pronto.
        // É multi-thread, e não de thread corrente, para as duas metades poderem esperar em threads
        // diferentes ao mesmo tempo.
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .thread_name("ir-ipc-pipe")
            .enable_io()
            .build()?;
        let cliente = {
            let _contexto = runtime.enter();
            ClientOptions::new().open(endereco)?
        };
        let (leitura, escrita) = tokio::io::split(cliente);
        // O runtime vive enquanto uma das metades viver.
        let runtime = Arc::new(runtime);
        Ok((
            Box::new(Escrita {
                runtime: Arc::clone(&runtime),
                metade: escrita,
            }),
            Box::new(Leitura {
                runtime,
                metade: leitura,
            }),
        ))
    }

    struct Leitura {
        runtime: Arc<Runtime>,
        metade: ReadHalf<NamedPipeClient>,
    }

    impl Read for Leitura {
        fn read(&mut self, destino: &mut [u8]) -> std::io::Result<usize> {
            let Self { runtime, metade } = self;
            runtime.block_on(metade.read(destino))
        }
    }

    struct Escrita {
        runtime: Arc<Runtime>,
        metade: WriteHalf<NamedPipeClient>,
    }

    impl Write for Escrita {
        fn write(&mut self, origem: &[u8]) -> std::io::Result<usize> {
            let Self { runtime, metade } = self;
            runtime.block_on(metade.write(origem))
        }

        fn flush(&mut self) -> std::io::Result<()> {
            let Self { runtime, metade } = self;
            runtime.block_on(metade.flush())
        }
    }
}

#[cfg(all(test, windows))]
mod tests {
    use std::io::{Read, Write};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    use tokio::io::AsyncReadExt;
    use tokio::net::windows::named_pipe::ServerOptions;

    use super::Duplex;

    /// Nomes únicos: dois testes no mesmo pipe se atrapalhariam, e o nome sobrevive entre execuções.
    static PROXIMO: AtomicUsize = AtomicUsize::new(0);

    /// Um serviço que aceita, lê tudo e nunca responde — o de verdade, entre dois avisos.
    struct ServidorCalado {
        nome: String,
        recebidos: Arc<AtomicUsize>,
        _runtime: tokio::runtime::Runtime,
    }

    impl ServidorCalado {
        fn subir() -> Self {
            let numero = PROXIMO.fetch_add(1, Ordering::SeqCst);
            let nome = format!(r"\\.\pipe\ir-ipc-teste-{}-{numero}", std::process::id());
            let runtime = tokio::runtime::Builder::new_multi_thread()
                .worker_threads(1)
                .enable_io()
                .build()
                .expect("runtime do servidor");
            let servidor = {
                let _contexto = runtime.enter();
                ServerOptions::new()
                    .first_pipe_instance(true)
                    .create(&nome)
                    .expect("cria o pipe")
            };
            let recebidos = Arc::new(AtomicUsize::new(0));
            let contagem = Arc::clone(&recebidos);
            runtime.spawn(async move {
                let mut servidor = servidor;
                if servidor.connect().await.is_err() {
                    return;
                }
                // Zero bytes é o cliente fechando; o laço para ali, e em erro também.
                let mut quadro = [0u8; 256];
                while let Ok(lidos @ 1..) = servidor.read(&mut quadro).await {
                    contagem.fetch_add(lidos, Ordering::SeqCst);
                }
            });
            Self {
                nome,
                recebidos,
                _runtime: runtime,
            }
        }

        /// Se a escrita chega ao serviço enquanto outra thread está presa esperando ler.
        ///
        /// É exatamente o que a janela e o agente fazem o tempo todo.
        fn escrita_chega_com_leitura_pendente(&self, duplex: Duplex) -> bool {
            let (mut escrita, mut leitura) = duplex;
            std::thread::spawn(move || {
                let _ = leitura.read(&mut [0u8; 4]);
            });
            // Dá tempo de a leitura entrar no sistema antes de escrever.
            std::thread::sleep(Duration::from_millis(300));

            let (feito_tx, feito) = std::sync::mpsc::channel();
            std::thread::spawn(move || {
                let _ = escrita
                    .write_all(&[1, 2, 3, 4])
                    .and_then(|()| escrita.flush());
                let _ = feito_tx.send(());
            });
            if feito.recv_timeout(Duration::from_secs(3)).is_err() {
                return false;
            }
            for _ in 0..30 {
                if self.recebidos.load(Ordering::SeqCst) >= 4 {
                    return true;
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            false
        }
    }

    #[test]
    fn escrever_nao_espera_a_leitura_pendente() {
        // A regressão do impasse: com uma thread presa esperando o serviço falar, um pedido da
        // janela ou um movimento de mouse do agente precisa chegar mesmo assim.
        let servidor = ServidorCalado::subir();
        let duplex = super::abrir(&servidor.nome).expect("abre o canal");
        assert!(
            servidor.escrita_chega_com_leitura_pendente(duplex),
            "a escrita ficou esperando a leitura pendente — o impasse voltou"
        );
    }

    #[test]
    fn o_handle_sincrono_da_biblioteca_padrao_trava_e_por_isso_nao_e_usado() {
        // A prova de que o teste acima mede o que diz medir: o mesmo roteiro, com o jeito antigo
        // de abrir o canal, trava. Se um dia o Windows deixar de serializar E/S síncrona, este
        // teste falha e avisa que a volta ao `std::fs::File` passou a ser possível.
        let servidor = ServidorCalado::subir();
        let escrita = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(&servidor.nome)
            .expect("abre o pipe como arquivo");
        let leitura = escrita.try_clone().expect("duplica o handle");
        let duplex: Duplex = (Box::new(escrita), Box::new(leitura));
        assert!(
            !servidor.escrita_chega_com_leitura_pendente(duplex),
            "com handle síncrono a escrita deveria esperar a leitura pendente"
        );
    }
}
