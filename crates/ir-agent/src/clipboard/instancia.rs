//! Um ajudante de clipboard por usuário.
//!
//! Quem sobe o ajudante agora é quem zela por ele — o serviço no Windows, o `systemd` do usuário no
//! Linux —, e um relançamento pode encontrar outro ainda vivo (o de um login antigo, um aberto à mão
//! para teste). Dois ajudantes ofereceriam a mesma cópia duas vezes e publicariam o que chega duas
//! vezes. O segundo, então, sai. A trava é de arquivo e o sistema a solta quando o processo morre,
//! de qualquer jeito que ele morra: não há trava velha para limpar.

use std::fs::{File, OpenOptions, TryLockError};
use std::path::{Path, PathBuf};

/// Tenta ser o único ajudante. `Ok(None)` quando outro já é.
///
/// A trava vale enquanto o [`File`] devolvido viver.
///
/// # Errors
///
/// Se o arquivo da trava não abrir. Quem chama segue sem trava: um ajudante a mais é melhor que
/// nenhum.
pub(crate) fn ser_o_unico() -> std::io::Result<Option<File>> {
    travar(&caminho())
}

/// A trava num caminho dado — separada para o teste não depender do diretório do usuário.
fn travar(caminho: &Path) -> std::io::Result<Option<File>> {
    if let Some(pasta) = caminho.parent() {
        std::fs::create_dir_all(pasta)?;
    }
    let arquivo = OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(caminho)?;
    match arquivo.try_lock() {
        Ok(()) => Ok(Some(arquivo)),
        Err(TryLockError::WouldBlock) => Ok(None),
        Err(TryLockError::Error(erro)) => Err(erro),
    }
}

/// Onde fica a trava: num diretório só deste usuário.
fn caminho() -> PathBuf {
    #[cfg(windows)]
    let pasta =
        std::env::var_os("LOCALAPPDATA").map(|base| PathBuf::from(base).join("InputRemote"));
    #[cfg(not(windows))]
    let pasta = std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from);
    pasta
        .unwrap_or_else(std::env::temp_dir)
        .join("inputremote-clipboard.lock")
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn o_segundo_sai_e_a_trava_vai_embora_com_o_primeiro() {
        let pasta = std::env::temp_dir().join(format!("ir-instancia-{}", std::process::id()));
        let caminho = pasta.join("trava.lock");

        let primeiro = travar(&caminho).unwrap().expect("o primeiro é o único");
        assert!(travar(&caminho).unwrap().is_none(), "o segundo não entra");
        drop(primeiro);
        assert!(
            travar(&caminho).unwrap().is_some(),
            "morto o primeiro, o próximo entra"
        );

        let _ = std::fs::remove_dir_all(pasta);
    }
}
