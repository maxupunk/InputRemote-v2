//! O ponto de escuta local, com um backend por sistema operacional.
//!
//! No Windows é um *named pipe*; no Linux, um socket de domínio Unix. Os dois entregam um fluxo
//! duplex por cliente que fala e ouve na mesma conexão — que é o que [`super::controle`] espera,
//! sem saber qual dos dois está por baixo.
//!
//! # Quem pode conectar é decisão do transporte
//!
//! E é decisão explícita, nunca herdada: um *pipe* criado por um serviço `LocalSystem` com o
//! descritor padrão **não** dá acesso ao usuário interativo, e o sintoma é a janela do próprio
//! dono da máquina levar "acesso negado" e cair para o modo de demonstração. Cada canal declara
//! o seu [`Acesso`], e os dois são diferentes de propósito.

use anyhow::{Context, Result};

/// Quem pode abrir este ponto de escuta.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Acesso {
    /// Só o serviço e os administradores.
    ///
    /// É o do canal do agente, que carrega injeção de entrada: se um processo qualquer do
    /// usuário pudesse abri-lo, qualquer programa que ele rodasse poderia digitar no prompt de
    /// UAC ([04, §5](../../../docs/04-seguranca.md)).
    Restrito,
    /// Também o usuário interativo, que é quem tem a janela na frente.
    UsuarioInterativo,
}

/// Só o serviço (`SY`) e os administradores (`BA`), com o DACL protegido contra herança.
#[cfg(windows)]
const SDDL_RESTRITO: &str = "D:P(A;;GA;;;SY)(A;;GA;;;BA)";
/// O anterior, mais leitura e escrita para o usuário interativo (`IU`).
#[cfg(windows)]
const SDDL_INTERATIVO: &str = "D:P(A;;GA;;;SY)(A;;GA;;;BA)(A;;GRGW;;;IU)";

#[cfg(windows)]
impl Acesso {
    /// A cadeia SDDL correspondente.
    const fn sddl(self) -> &'static str {
        match self {
            Self::Restrito => SDDL_RESTRITO,
            Self::UsuarioInterativo => SDDL_INTERATIVO,
        }
    }
}

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
    /// Mantido vivo enquanto houver escuta: cada instância nova do *pipe* é criada com ele.
    seguranca: super::seguranca::Descritor,
    proximo: tokio::net::windows::named_pipe::NamedPipeServer,
}

#[cfg(windows)]
impl Escuta {
    /// Abre o ponto de escuta com o nome dado (por exemplo `\\.\pipe\inputremote-control`).
    ///
    /// # Errors
    ///
    /// Erro do sistema se o nome já estiver em uso ou o processo não puder criar o *pipe*.
    pub(crate) fn abrir(nome: &str, acesso: Acesso) -> Result<Self> {
        let mut seguranca = super::seguranca::Descritor::de_sddl(acesso.sddl())?;
        let proximo = Self::criar(nome, &mut seguranca, true)?;
        Ok(Self {
            nome: nome.to_owned(),
            seguranca,
            proximo,
        })
    }

    /// Cria uma instância do *pipe* com o descritor de segurança do canal.
    fn criar(
        nome: &str,
        seguranca: &mut super::seguranca::Descritor,
        primeira: bool,
    ) -> Result<tokio::net::windows::named_pipe::NamedPipeServer> {
        seguranca
            .criar_pipe(nome, primeira)
            .with_context(|| format!("criando o pipe {nome}"))
    }

    /// Espera o próximo cliente e devolve a conexão dele.
    ///
    /// # Errors
    ///
    /// Erro do sistema ao conectar ou ao preparar a instância seguinte do *pipe*.
    pub(crate) async fn aceitar(&mut self) -> Result<Conexao> {
        // Espera este cliente, depois já prepara a instância seguinte antes de servir: é o que
        // permite um segundo cliente conectar enquanto o primeiro é atendido.
        self.proximo.connect().await.context("aguardando cliente")?;
        let seguinte = Self::criar(&self.nome, &mut self.seguranca, false)
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
    pub(crate) fn abrir(caminho: &str, acesso: Acesso) -> Result<Self> {
        use std::os::unix::fs::PermissionsExt;
        if let Some(pai) = std::path::Path::new(caminho).parent() {
            std::fs::create_dir_all(pai).with_context(|| format!("criando {}", pai.display()))?;
        }
        // Um socket de uma execução anterior impede o `bind`; removê-lo é seguro porque somos o
        // dono do caminho.
        let _ = std::fs::remove_file(caminho);
        let listener = tokio::net::UnixListener::bind(caminho)
            .with_context(|| format!("vinculando {caminho}"))?;
        // O canal do agente não tem por que ser alcançável nem pelo grupo; o de controle precisa
        // do grupo do serviço, que é por onde a interface entra.
        let modo = match acesso {
            Acesso::Restrito => 0o600,
            Acesso::UsuarioInterativo => 0o660,
        };
        let _ = std::fs::set_permissions(caminho, std::fs::Permissions::from_mode(modo));
        // `0660` sozinho não basta: o socket nasce `root:root`, e o grupo precisa ser um a que o
        // usuário da janela pertença. Sem isto a interface leva "permissão negada".
        if acesso == Acesso::UsuarioInterativo {
            super::grupo::dar_ao_grupo(caminho);
        }
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
