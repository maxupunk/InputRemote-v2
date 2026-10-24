//! O enlace com o painel no Android ([12](../../../docs/12-painel-android.md)).
//!
//! Puro, como o núcleo: nada de socket, de relógio de parede nem de `tokio`. As duas pontas usam
//! este crate — o serviço do computador que agrega (pelo `ir-monitor-rede`) e o aplicativo
//! Android (pelo `ir-monitor-android`) —, então o formato, a criptografia e os limites de alerta
//! existem **uma vez**.
//!
//! | Módulo | O quê |
//! |---|---|
//! | [`mensagem`] | o que o painel e o computador dizem um ao outro |
//! | [`detalhes`] | a cópia em curso e os processos, ao lado do quadro |
//! | [`processos`] | os processos agrupados pelo nome, e o que se quer saber deles |
//! | [`entre_pcs`] | o recado do painel entre os dois computadores (`Control::Panel`) |
//! | [`conversa`] | o aperto de mão Noise, o código de seis dígitos e os quadros cifrados |
//! | [`mesa`] | o computador juntando a própria amostra, a do par e a latência num [`Quadro`] |
//! | [`vista`] | o quadro pronto para desenhar: textos, frações e níveis |
//! | [`nivel`] | quando um número fica âmbar ou vermelho |
//!
//! # Segurança
//!
//! O painel é um par como outro qualquer: mesmo Noise, mesmo código de seis dígitos comparado nas
//! duas telas ([04, §3](../../../docs/04-seguranca.md)). A diferença é o que ele pode: **só ler**.
//! O computador só entende [`DoPainel`](mensagem::DoPainel) vindo dele, e esse vocabulário não tem
//! como pedir entrada, clipboard ou arquivo.

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

pub mod conversa;
pub mod detalhes;
pub mod entre_pcs;
pub mod mensagem;
pub mod mesa;
pub mod nivel;
pub mod processos;
pub mod vista;

pub use conversa::{Conversa, Evento, Falha, Politica};
pub use detalhes::{Copia, Detalhes, FaseDaCopia, Interesse};
pub use entre_pcs::EntrePcs;
pub use mensagem::{
    Apresentacao, DoPainel, Enlace, Maquina, ParaPainel, Quadro, Rota, Situacao, VERSAO,
};
pub use mesa::Mesa;
pub use nivel::Nivel;
pub use processos::{InteresseEmProcessos, Processos, UsoDeProcesso};
pub use vista::Vista;

/// A porta TCP em que o serviço atende o painel.
///
/// Vizinha da do produto (52525), e fixa: o painel descobre o computador pela descoberta de sempre
/// (UDP 52524), que conta a porta da sessão, e não a desta. Decisão de 2026-10-24.
pub const PORTA: u16 = 52527;
