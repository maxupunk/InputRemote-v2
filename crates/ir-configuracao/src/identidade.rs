//! A identidade da máquina: a chave gravada, só do dono, e a chave dos pares em texto.

use std::path::Path;

use anyhow::{Context, Result};
use ir_crypto::{Identity, PublicKey};

use crate::gravar_atomico;

/// Carrega a identidade da máquina, gerando e gravando na primeira vez.
///
/// # Errors
///
/// Erro de E/S, ou material de chave inválido no arquivo.
pub fn load_identity(dir: &Path) -> Result<Identity> {
    let path = dir.join("identity.key");
    if path.exists() {
        // Uma chave gravada por uma versão antiga, ou mexida à mão, volta a ser só do dono.
        restrict(&path)?;
        let bytes = std::fs::read(&path).context("lendo identidade")?;
        return Identity::from_secret_bytes(&bytes).context("identidade inválida");
    }
    let identity = Identity::generate();
    gravar_atomico(
        dir,
        "identity.key",
        identity.secret_bytes().as_bytes(),
        true,
    )
    .context("gravando identidade")?;
    Ok(identity)
}

/// Restringe ao dono a permissão de um arquivo de chave que já existe.
///
/// # Errors
///
/// Erro de E/S se a permissão não puder ser trocada: seguir com a chave aberta seria pior.
#[cfg_attr(not(unix), allow(clippy::unnecessary_wraps))]
fn restrict(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let atual = std::fs::metadata(path)
            .context("lendo a permissão da identidade")?
            .permissions()
            .mode();
        if atual & 0o077 != 0 {
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
                .context("restringindo a identidade ao dono")?;
        }
    }
    #[cfg(not(unix))]
    {
        let _ = path;
    }
    Ok(())
}

/// Grava a chave pública de um par em hexadecimal.
#[must_use]
pub fn encode_key(key: &PublicKey) -> String {
    key.hex()
}

/// Lê uma chave pública de 64 hexadecimais.
#[must_use]
pub fn decode_key(text: &str) -> Option<PublicKey> {
    PublicKey::de_hex(text)
}
