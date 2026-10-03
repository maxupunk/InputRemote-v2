//! O seletor de pasta do sistema, fora da janela.
//!
//! Numa thread própria: o seletor é modal e bloqueia quem o abre, e a janela precisa continuar
//! desenhando e recolhendo avisos enquanto ele está aberto. A resposta volta por um canal que a
//! batida da janela confere.
//!
//! - **Windows**: o diálogo nativo (`IFileOpenDialog`), pelo `rfd` — sem dependência a mais.
//! - **Linux**: o `zenity`, que o GNOME instala e que já fala com o portal do ambiente gráfico. O
//!   `rfd` no Linux traria um runtime assíncrono inteiro para abrir um diálogo.

use std::sync::mpsc::{self, Receiver};

/// Abre o seletor. A resposta é o caminho escolhido, ou `None` se a pessoa cancelar.
pub(crate) fn abrir() -> Receiver<Option<String>> {
    let (resposta, recebe) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = resposta.send(escolher());
    });
    recebe
}

#[cfg(windows)]
fn escolher() -> Option<String> {
    rfd::FileDialog::new()
        .set_title("Escolha a pasta para compartilhar com o outro computador")
        .pick_folder()
        .map(|caminho| caminho.to_string_lossy().into_owned())
}

#[cfg(not(windows))]
fn escolher() -> Option<String> {
    let saida = std::process::Command::new("zenity")
        .args([
            "--file-selection",
            "--directory",
            "--title=Escolha a pasta para compartilhar com o outro computador",
        ])
        .output()
        .ok()?;
    if !saida.status.success() {
        return None;
    }
    let caminho = String::from_utf8(saida.stdout).ok()?;
    let caminho = caminho.trim_end_matches(['\n', '\r']);
    (!caminho.is_empty()).then(|| caminho.to_owned())
}
