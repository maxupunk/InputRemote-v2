//! Configuração e identidade persistentes da máquina.
//!
//! Tudo pertence à máquina, não ao usuário ([02, §7](../../../docs/02-arquitetura.md)). Para o
//! teste em primeiro plano, o diretório de estado vem de `IR_DATA_DIR` (padrão `./ir-state`), o
//! que evita depender de privilégio antes da instalação como serviço.

#![allow(unreachable_pub)]

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use ir_crypto::{Identity, PublicKey};
use ir_proto::screens::Edge;
use ir_session::Role;
use serde::{Deserialize, Serialize};

/// A configuração da máquina, como fica no `config.toml`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// `"server"` (tem o teclado) ou `"client"` (é controlada).
    pub role: String,
    /// A borda que dá para o par: `left`/`right`/`top`/`bottom`.
    pub peer_edge: String,
    /// A porta UDP local.
    pub port: u16,
    /// Largura da tela em pixels (usada quando a plataforma não informa).
    pub screen_width: u32,
    /// Altura da tela em pixels.
    pub screen_height: u32,
    /// O endereço do par, para iniciar a conexão. Opcional.
    ///
    /// Duas formas, e é ela que decide o portador: `10.0.0.135:52525` fala pela rede,
    /// `AC:50:DE:47:EB:28` fala pelo Bluetooth. As duas não se confundem — um `ip:porta` tem dois
    /// grupos separados por `:`, um endereço de rádio tem seis —, e é por isso que um campo de
    /// texto só dá conta dos dois sem o arquivo de configuração mudar de formato.
    pub peer_addr: Option<String>,
    /// Pares já pareados.
    #[serde(default)]
    pub peers: Vec<PinnedPeer>,
    /// Onde os arquivos recebidos ficam. Vazio significa `<estado>/recebidos`.
    ///
    /// Configurável porque a pasta de estado pode estar num disco pequeno, e uma transferência de
    /// 5 GB não deve ser obrigada a caber junto com a configuração.
    #[serde(default)]
    pub recebidos: Option<String>,
}

/// Um par pareado, com a chave estática fixada.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PinnedPeer {
    /// A chave pública, em hexadecimal.
    pub pubkey: String,
    /// O último endereço conhecido, se houver.
    pub addr: Option<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            role: "server".to_owned(),
            peer_edge: "right".to_owned(),
            port: 52525,
            screen_width: 1920,
            screen_height: 1080,
            peer_addr: None,
            peers: Vec::new(),
            recebidos: None,
        }
    }
}

impl Config {
    /// O papel da sessão.
    ///
    /// # Errors
    ///
    /// Erro se o texto não for `server` nem `client`.
    pub fn session_role(&self) -> Result<Role> {
        match self.role.as_str() {
            "server" => Ok(Role::Server),
            "client" => Ok(Role::Client),
            other => bail!("papel inválido: {other} (use server ou client)"),
        }
    }

    /// A borda de travessia.
    ///
    /// # Errors
    ///
    /// Erro se o texto não nomear uma borda.
    pub fn edge(&self) -> Result<Edge> {
        match self.peer_edge.as_str() {
            "left" => Ok(Edge::Left),
            "right" => Ok(Edge::Right),
            "top" => Ok(Edge::Top),
            "bottom" => Ok(Edge::Bottom),
            other => bail!("borda inválida: {other}"),
        }
    }

    /// Onde os arquivos recebidos ficam.
    #[must_use]
    pub fn pasta_de_recebidos(&self, dir: &Path) -> PathBuf {
        self.recebidos
            .as_deref()
            .filter(|texto| !texto.trim().is_empty())
            .map_or_else(|| dir.join("recebidos"), PathBuf::from)
    }

    /// A primeira chave de par fixada, se houver.
    #[must_use]
    pub fn first_peer_key(&self) -> Option<PublicKey> {
        self.peers.first().and_then(|p| decode_key(&p.pubkey))
    }

    /// Grava a configuração no diretório de estado, de forma atômica.
    ///
    /// # Errors
    ///
    /// Erro de E/S ao escrever.
    pub fn save(&self, dir: &Path) -> Result<()> {
        let text = toml::to_string_pretty(self).context("serializando config")?;
        let path = dir.join("config.toml");
        let tmp = dir.join("config.toml.tmp");
        std::fs::write(&tmp, text).context("gravando config temporária")?;
        std::fs::rename(&tmp, &path).context("trocando config")?;
        Ok(())
    }
}

/// O diretório de estado, de `IR_DATA_DIR` ou `./ir-state`.
#[must_use]
pub fn data_dir() -> PathBuf {
    std::env::var_os("IR_DATA_DIR").map_or_else(|| PathBuf::from("ir-state"), PathBuf::from)
}

/// Carrega a configuração, criando o padrão se ainda não existir.
///
/// # Errors
///
/// Erro de E/S ou de formato ao ler o `config.toml`.
pub fn load_config(dir: &Path) -> Result<Config> {
    std::fs::create_dir_all(dir).context("criando o diretório de estado")?;
    let path = dir.join("config.toml");
    if !path.exists() {
        let config = Config::default();
        config.save(dir)?;
        return Ok(config);
    }
    let text = std::fs::read_to_string(&path).context("lendo config")?;
    toml::from_str(&text).context("interpretando config")
}

/// Carrega a identidade da máquina, gerando e gravando na primeira vez.
///
/// # Errors
///
/// Erro de E/S, ou material de chave inválido no arquivo.
pub fn load_identity(dir: &Path) -> Result<Identity> {
    let path = dir.join("identity.key");
    if path.exists() {
        let bytes = std::fs::read(&path).context("lendo identidade")?;
        return Identity::from_secret_bytes(&bytes).context("identidade inválida");
    }
    let identity = Identity::generate();
    let tmp = dir.join("identity.key.tmp");
    std::fs::write(&tmp, identity.secret_bytes().as_bytes()).context("gravando identidade")?;
    restrict(&tmp);
    std::fs::rename(&tmp, &path).context("trocando identidade")?;
    Ok(identity)
}

/// Restringe a permissão do arquivo de chave ao dono, onde a plataforma permite.
fn restrict(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
    }
    #[cfg(not(unix))]
    {
        let _ = path;
    }
}

/// Grava a chave pública de um par em hexadecimal.
#[must_use]
pub fn encode_key(key: &PublicKey) -> String {
    use core::fmt::Write;
    let mut out = String::with_capacity(64);
    for byte in key.0 {
        let _ = write!(out, "{byte:02x}");
    }
    out
}

/// Lê uma chave pública de 64 hexadecimais.
#[must_use]
pub fn decode_key(text: &str) -> Option<PublicKey> {
    if text.len() != 64 {
        return None;
    }
    let mut bytes = [0u8; 32];
    for (slot, pair) in bytes.iter_mut().zip(text.as_bytes().chunks_exact(2)) {
        let hi = (pair.first().copied()?) as char;
        let lo = (pair.get(1).copied()?) as char;
        let hi = u8::try_from(hi.to_digit(16)?).ok()?;
        let lo = u8::try_from(lo.to_digit(16)?).ok()?;
        *slot = (hi << 4) | lo;
    }
    Some(PublicKey(bytes))
}
