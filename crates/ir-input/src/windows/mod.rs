//! O backend Windows: captura por ganchos de baixo nível, injeção por `SendInput`.

#![allow(unreachable_pub)]

pub mod desktops;
pub mod hooks;
pub mod scancode;
pub mod sendinput;
pub mod telas;

/// O tamanho da tela primária, em pixels.
#[cfg(windows)]
pub(crate) fn primary_screen_size() -> Option<(u32, u32)> {
    #![allow(unsafe_code)]
    use windows::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_CXSCREEN, SM_CYSCREEN};
    // SAFETY: `GetSystemMetrics` recebe um índice e devolve um inteiro, sem pré-condição.
    let (w, h) = unsafe { (GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN)) };
    match (u32::try_from(w), u32::try_from(h)) {
        (Ok(w), Ok(h)) if w > 0 && h > 0 => Some((w, h)),
        _ => None,
    }
}
