//! O Bluetooth que parou de alcançar o par enquanto a rede o alcança: perceber, religar o rádio
//! deste computador, e dizer à janela o que está acontecendo.
//!
//! A controladora do rádio pode travar sem cair. Na bancada (log 76), a do Fedora parou de escoar
//! no meio de uma sessão e, dali em diante, não alcançava mais o Windows — nem o InputRemote, nem o
//! áudio do próprio sistema (`ENOSYS` nos dois). Vinte minutos depois continuava assim; desligar e
//! ligar o rádio a trouxe de volta na hora. Ninguém precisa saber disso para usar o produto: com a
//! rede provando que o outro computador está de pé, o serviço religa o rádio sozinho.
//!
//! # A salvaguarda
//!
//! Religar o rádio derruba por alguns segundos todo aparelho Bluetooth desta máquina. Por isso:
//!
//! - só quando o Bluetooth está fora **e a rede de pé** há [`PRAZO`] — a rede é a prova de que o
//!   par está ligado e com o serviço no ar, e de que o defeito é do rádio;
//! - uma vez só por queda: se não voltar, o aviso diz isso e oferece o botão, e nada mais se faz
//!   sozinho até o Bluetooth voltar;
//! - nunca sozinho com outro aparelho Bluetooth conectado: aí a escolha é da pessoa, pelo botão.
//!
//! As decisões moram no [`Vigia`], que não toca o sistema e é testado com relógio de mentira; o que
//! toca o sistema mora atrás de [`Adaptador`].

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

mod adaptador;
mod vigia;

pub use adaptador::{Adaptador, contar_aparelhos, do_sistema};
pub use vigia::{Enlaces, PRAZO, Vigia};
