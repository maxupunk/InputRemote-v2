//! O vocabulário do núcleo: o que entra e o que sai.
//!
//! `ir-session` é uma função sobre estado. [`Input`] é tudo que pode acontecer com a sessão;
//! [`Command`] é tudo que a sessão pode pedir. Não há mais nada — nenhuma chamada de sistema,
//! nenhum socket, nenhum relógio lido.
//!
//! O instante **não** faz parte de [`Input`]: ele é parâmetro de
//! [`Session::step`](crate::Session::step), porque todo evento acontece em algum momento e
//! repetir o campo em cada variante só produziria ruído.

mod clip_text;
mod command;
mod input;
mod notice;

pub use clip_text::ClipText;
pub use command::{Command, CommandBatch, Injection, TimerId};
pub use input::{Input, LinkDown};
pub use notice::{CarrierChoice, Notice};
