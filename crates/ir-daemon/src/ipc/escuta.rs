//! O ponto de escuta local, com um backend por sistema operacional.
//!
//! No Windows é um *named pipe*; no Linux, um socket de domínio Unix. Os dois entregam um fluxo
//! duplex por cliente que fala e ouve na mesma conexão — que é o que [`super::controle`] espera,
//! sem saber qual dos dois está por baixo.
//!
//! # Segurança
//!
//! A restrição de quem pode conectar é do transporte, não da lógica: no Linux, o socket nasce
//! `0660` para o grupo do serviço; no Windows, o *pipe* usa o descritor padrão (o criador e os
//! administradores), o que basta enquanto serviço e interface rodam sob o mesmo usuário no teste
//! em primeiro plano. O endurecimento por SDDL, para o serviço rodando como SYSTEM, é da etapa
//! de instalação.

use anyhow::{Context, Result};

/// Uma conexão duplex já aceita.
#[cfg(windows)]
pub(crate) type Conexao = tokio::net::windows::named_pipe::NamedPipeServer;
/// Uma conexão duplex já aceita.
#[cfg(not(windows))]
pub(crate) type Conexao = tokio::net::UnixStream;

/// O ponto de escuta, que aceita uma conexão de cada vez.
#[cfg(windows)]
pub(crate) struct Escuta {
    nome: String,
    proximo: tokio::net::windows::named_pipe::NamedPipeServer,
}

#[cfg(windows)]
impl Escuta {
    /// Abre o ponto de escuta com o nome dado (por exemplo `\\.\pipe\inputremote-control`).
    ///
    /// # Errors
    ///
    /// Erro do sistema se o nome já estiver em uso ou o processo não puder criar o *pipe*.
    pub(crate) fn abrir(nome: &str) -> Result<Self> {
        use tokio::net::windows::named_pipe::ServerOptions;
        let proximo = ServerOptions::new()
            .first_pipe_instance(true)
            .create(nome)
            .with_context(|| format!("criando o pipe {nome}"))?;
        Ok(Self {
            nome: nome.to_owned(),
            proximo,
        })
    }

    /// Espera o próximo cliente e devolve a conexão dele.
    ///
    /// # Errors
    ///
    /// Erro do sistema ao conectar ou ao preparar a instância seguinte do *pipe*.
    pub(crate) async fn aceitar(&mut self) -> Result<Conexao> {
        use tokio::net::windows::named_pipe::ServerOptions;
        // Espera este cliente, depois já prepara a instância seguinte antes de servir: é o que
        // permite um segundo cliente conectar enquanto o primeiro é atendido.
        self.proximo.connect().await.context("aguardando cliente")?;
        let seguinte = ServerOptions::new()
            .create(&self.nome)
            .context("preparando o pipe seguinte")?;
        Ok(std::mem::replace(&mut self.proximo, seguinte))
    }
}

/// O ponto de escuta, que aceita uma conexão de cada vez.
#[cfg(not(windows))]
pub(crate) struct Escuta {
    listener: tokio::net::UnixListener,
}

#[cfg(not(windows))]
impl Escuta {
    /// Abre o ponto de escuta no caminho dado (por exemplo `/run/inputremote/control.sock`).
    ///
    /// # Errors
    ///
    /// Erro de E/S ao criar o diretório, remover um socket velho ou vincular o novo.
    pub(crate) fn abrir(caminho: &str) -> Result<Self> {
        use std::os::unix::fs::PermissionsExt;
        if let Some(pai) = std::path::Path::new(caminho).parent() {
            std::fs::create_dir_all(pai).with_context(|| format!("criando {}", pai.display()))?;
        }
        // Um socket de uma execução anterior impede o `bind`; removê-lo é seguro porque somos o
        // dono do caminho.
        let _ = std::fs::remove_file(caminho);
        let listener = tokio::net::UnixListener::bind(caminho)
            .with_context(|| format!("vinculando {caminho}"))?;
        let _ = std::fs::set_permissions(caminho, std::fs::Permissions::from_mode(0o660));
        Ok(Self { listener })
    }

    /// Espera o próximo cliente e devolve a conexão dele.
    ///
    /// # Errors
    ///
    /// Erro de E/S ao aceitar.
    pub(crate) async fn aceitar(&mut self) -> Result<Conexao> {
        let (conexao, _) = self.listener.accept().await.context("aceitando conexão")?;
        Ok(conexao)
    }
}
