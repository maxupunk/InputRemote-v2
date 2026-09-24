//! O backend Linux: injeção por `uinput`, e captura por `evdev`.

#![allow(unreachable_pub)]

mod aceleracao;
pub mod captura;
pub mod keymap;
mod touchpad;
mod traducao;
pub mod uinput;
