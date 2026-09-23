//! Entrega confiável e ordenada dos canais de controle, entrada, retorno e texto, sobre um caminho
//! que pode perder, duplicar e reordenar.
//!
//! É o mínimo que `docs/03-protocolo.md` §4.1 descreve, e não uma reimplementação de TCP:
//!
//! - sequência por canal;
//! - confirmação cumulativa mais bitmap das 32 anteriores;
//! - retransmissão com intervalo que dobra a cada reenvio, até um quarto do prazo de queda;
//! - uma mensagem sem confirmação além do prazo de queda derruba o enlace.
//!
//! **Por que cair em vez de continuar.** Passado o prazo, prosseguir significaria seguir com uma
//! lacuna no canal de teclado. Se a mensagem perdida for um `KeyUp`, a tecla fica presa na máquina
//! do outro. Cair é ruim; tecla presa é pior, porque o usuário não sabe o que aconteceu nem como
//! sair.
//!
//! # Sobre qualquer portador
//!
//! Nasceu para o UDP, e desde a versão 2 do protocolo vale para toda rota de entrada — RFCOMM
//! sozinho, UDP sozinho, ou os dois juntos na rota dupla, que duplica **todo** quadro
//! ([ADR-0012](../../../docs/adr/0012-rota-dupla.md)). A detecção de repetição do receptor é o que
//! descarta a cópia que chega depois.
//!
//! # Por que é um crate
//!
//! Saiu do `ir-session` quando a rota dupla levou a sessão além do teto de tamanho de crate
//! (`docs/09-padroes-de-codigo.md` §1). A fronteira já estava provada: o módulo só conhecia o
//! `ir-proto` e o tempo injetado, e tinha testes próprios. Continua puro — sem relógio, sem E/S —,
//! e o `ir-session` o reexporta como `reliability` e `time`, então nada fora dele mudou de nome.

#![forbid(unsafe_code)]
#![cfg_attr(
    test,
    allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::indexing_slicing,
        clippy::panic
    )
)]

mod channels;
mod receiver;
mod sender;
pub mod sequences;
pub mod time;

pub use channels::{Due, ReliableChannels};
pub use receiver::{Delivery, Receiver};
pub use sender::{ACK_REACH, SendOutcome, Sender, TimeoutOutcome, WINDOW};
pub use sequences::Sequences;
