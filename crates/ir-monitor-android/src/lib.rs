//! O núcleo do painel no Android ([12](../../../docs/12-painel-android.md)).
//!
//! Tudo o que não é tela: achar o computador na rede, conversar com ele pela mesma
//! [`Conversa`](ir_monitor::Conversa) que o serviço usa, guardar a identidade do aparelho e os
//! computadores pareados, e entregar a [`Vista`](ir_monitor::Vista) pronta. O aplicativo Kotlin só
//! desenha o que recebe e encaminha os toques ([`jni`]).
//!
//! | Módulo | O quê |
//! |---|---|
//! | [`cofre`] | a identidade do aparelho e os computadores pareados, num arquivo |
//! | [`descoberta`] | a pergunta "quem está aí?" de sempre, na UDP 52524 |
//! | [`cliente`] | a ligação com um computador, numa linha própria |
//! | `montagem` | o quadro e os detalhes de cada segundo juntos numa vista |
//! | [`ponte`] | os eventos em JSON, o contrato com o Kotlin |
//! | [`jni`] | as funções que o Kotlin chama |

#![cfg_attr(
    test,
    allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::indexing_slicing,
        clippy::panic
    )
)]

pub mod cliente;
pub mod cofre;
pub mod descoberta;
pub mod jni;
mod montagem;
pub mod ponte;

use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::Duration;

use cliente::{Comando, Destino, Evento, Partes};

/// Uma ligação com um computador, vista de fora da linha que a conduz.
#[derive(Debug)]
pub struct Ligacao {
    comandos: Sender<Comando>,
    eventos: Mutex<Receiver<Evento>>,
}

impl Ligacao {
    /// Começa a ligar, numa linha própria.
    ///
    /// # Errors
    ///
    /// Os do sistema ao criar a linha.
    pub fn abrir(
        pasta: PathBuf,
        nome_do_aparelho: String,
        destino: Destino,
    ) -> std::io::Result<Self> {
        let (comandos, recebe_comandos) = mpsc::channel();
        let (manda_eventos, eventos) = mpsc::channel();
        let partes = Partes {
            pasta,
            nome_do_aparelho,
            destino,
            comandos: recebe_comandos,
            eventos: manda_eventos,
        };
        std::thread::Builder::new()
            .name("ir-painel".to_owned())
            .spawn(move || cliente::rodar(partes))?;
        Ok(Self {
            comandos,
            eventos: Mutex::new(eventos),
        })
    }

    /// Um pedido à linha. Depois de ela acabar, não tem efeito.
    pub fn comandar(&self, comando: Comando) {
        let _ = self.comandos.send(comando);
    }

    /// O próximo evento, esperando até `espera`. `None` sem evento, ou com a linha encerrada e
    /// tudo já entregue.
    pub fn proximo(&self, espera: Duration) -> Option<Evento> {
        self.eventos.lock().ok()?.recv_timeout(espera).ok()
    }
}

impl Drop for Ligacao {
    fn drop(&mut self) {
        self.comandar(Comando::Fechar);
    }
}
