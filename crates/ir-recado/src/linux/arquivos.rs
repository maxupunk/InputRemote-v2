//! Mostrar no gerenciador de arquivos o que chegou: a pasta aberta, com o item já selecionado.
//!
//! Pelo `org.freedesktop.FileManager1.ShowItems`, que o Nautilus, o Dolphin e o Nemo atendem — o
//! mesmo caminho do "Mostrar na pasta" dos navegadores. Abrir só a pasta (`xdg-open`) obrigaria a
//! pessoa a procurar o arquivo entre os outros recebidos; é o que sobra quando ninguém atende.

use std::fmt::Write as _;
use std::os::unix::ffi::OsStrExt;
use std::path::Path;
use std::process::Command;

/// Abre a pasta do item, com ele selecionado.
pub fn mostrar(item: &Path) {
    if let Err(erro) = mostrar_pelo_barramento(item) {
        tracing::debug!(%erro, "sem FileManager1; abrindo só a pasta");
        if let Some(pasta) = item.parent() {
            let _ = Command::new("xdg-open").arg(pasta).spawn();
        }
    }
}

fn mostrar_pelo_barramento(item: &Path) -> zbus::Result<()> {
    let conexao = zbus::blocking::Connection::session()?;
    conexao.call_method(
        Some("org.freedesktop.FileManager1"),
        "/org/freedesktop/FileManager1",
        Some("org.freedesktop.FileManager1"),
        "ShowItems",
        &(vec![uri(item)], ""),
    )?;
    Ok(())
}

/// O endereço `file://` do caminho, com o que não é letra, número ou separador escapado.
#[must_use]
pub fn uri(caminho: &Path) -> String {
    let mut uri = String::from("file://");
    for &byte in caminho.as_os_str().as_bytes() {
        if byte.is_ascii_alphanumeric() || b"-._~/".contains(&byte) {
            uri.push(char::from(byte));
        } else {
            let _ = write!(uri, "%{byte:02X}");
        }
    }
    uri
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn o_endereco_escapa_espaco_e_acento() {
        assert_eq!(
            uri(Path::new("/home/ana/Área de Trabalho/a#1.txt")),
            "file:///home/ana/%C3%81rea%20de%20Trabalho/a%231.txt"
        );
    }
}
