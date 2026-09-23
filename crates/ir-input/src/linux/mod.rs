//! O backend Linux: injeção por `uinput`, e captura por `evdev`.

#![allow(unreachable_pub)]

pub mod captura;
pub mod keymap;
pub mod uinput;
