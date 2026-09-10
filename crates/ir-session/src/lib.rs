//! A máquina de estados do produto InputRemote.
//!
//! Crate **puro**: nenhuma E/S, nenhum relógio lido, nenhuma API de sistema operacional,
//! nenhum `async`, nenhum estado compartilhado. Entra um evento, sai uma lista de comandos.
//!
//! ```
//! use ir_session::{CommandBatch, Input, Session, SessionConfig, LocalIdentity, Timestamp};
//! use ir_proto::{carrier::Carrier, ids::MachineId, peer::{Capabilities, MachineName}};
//! use ir_proto::screens::Edge;
//!
//! let identity = LocalIdentity {
//!     machine: MachineId([1; 16]),
//!     name: MachineName::new("bancada").unwrap(),
//!     capabilities: Capabilities::default(),
//! };
//! let mut session = Session::new(SessionConfig::server(Edge::Right), identity);
//! let mut out = CommandBatch::new();
//!
//! session.step(Timestamp::ZERO, Input::CarrierUp(Carrier::Udp), &mut out);
//! assert!(!out.is_empty(), "subir um portador começa o handshake");
//! ```
//!
//! # Por que assim
//!
//! O InputRemote 1 tinha a lógica de produto dentro do crate da interface, misturada com
//! `async`, sockets e o ciclo de repintura da janela — 8.304 linhas em dois arquivos. Um bug
//! de "tecla presa depois de reconectar" só podia ser reproduzido com dois computadores, um
//! rádio e a sequência exata de eventos, o que na prática significava que não virava teste.
//!
//! Aqui, o mesmo bug é uma sequência de [`Input`] num teste que roda em microssegundos.
//! Ver [ADR-0004](https://github.com/inputremote/inputremote/blob/main/docs/adr/0004-nucleo-sans-io.md).
//!
//! # O que este crate garante
//!
//! - **Nada fica pressionado.** Toda queda, troca de portador, perda de agente, emergência e
//!   encerramento emite [`Command::ReleaseAll`] antes de qualquer outra coisa.
//! - **A reconciliação é idempotente.** Aplicar o mesmo estado duas vezes não produz ação na
//!   segunda, o que permite o snapshot periódico sem risco de oscilação.
//! - **A escolha de portador é única e visível.** Uma política só, e o motivo da escolha vai
//!   para a interface junto com o resultado.

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

pub mod config;
pub mod event;
pub mod phase;
pub mod sequences;
pub mod session;
pub mod time;

pub use config::{Role, SessionConfig, Timings};
pub use event::{
    CarrierChoice, Command, CommandBatch, Injection, Input, LinkDown, Notice, TimerId,
};
pub use phase::Phase;
pub use session::{CarrierSet, ConfigError, LocalIdentity, PeerInfo, Session};
pub use time::{Millis, Timestamp};
