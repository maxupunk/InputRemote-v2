//! O backend Windows: captura por ganchos de baixo nível, injeção por `SendInput`.

#![allow(unreachable_pub)]

pub mod hooks;
pub mod scancode;
pub mod sendinput;
