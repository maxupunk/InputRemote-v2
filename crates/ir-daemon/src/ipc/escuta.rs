//! O ponto de escuta local, com um backend por sistema operacional.
//!
//! No Windows é um *named pipe*; no Linux, um socket de domínio Unix. Os dois entregam um fluxo
//! duplex por cliente que fala e ouve na mesma conexão — que é o que [`super::controle`] espera,
//! sem saber qual dos dois está por baixo.
//!
//! # Quem pode conectar é decisão do transporte
//!
//! E é decisão explícita, nunca herdada. Cada canal declara o seu [`Acesso`], e cada conexão
//! aceita vem com a [`Chamada`] que diz se quem conectou pode usá-lo.
//!
//! - No **Windows**, o descritor de segurança do *pipe* barra quem não pode no próprio sistema, e
//!   o que chega até aqui já é permitido.
//! - No **Linux**, o portão é o serviço: ele lê a credencial de quem conectou (`SO_PEERCRED`) e
//!   consulta a filiação ao grupo no banco de usuários **na hora** ([`ir_acesso::porteiro`]). A
//!   permissão do arquivo não serve de portão porque é conferida com os grupos do processo que
//!   conecta — e no GNOME a sessão gráfica carrega os grupos de quando nasceu, então um `usermod`
//!   só alcançaria a janela depois de reiniciar a máquina.

use anyhow::{Context, Result};

pub(crate) use ir_acesso::{Acesso, Chamada};

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
    seguranca: ir_acesso::seguranca::Descritor,
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
        let mut seguranca = ir_acesso::seguranca::Descritor::de_sddl(acesso.sddl())?;
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
        seguranca: &mut ir_acesso::seguranca::Descritor,
        primeira: bool,
    ) -> Result<tokio::net::windows::named_pipe::NamedPipeServer> {
        seguranca
            .criar_pipe(nome, primeira)
            .with_context(|| format!("criando o pipe {nome}"))
    }

    /// Espera o próximo cliente e devolve a conexão dele, com a decisão sobre ela.
    ///
    /// # Errors
    ///
    /// Erro do sistema ao conectar ou ao preparar a instância seguinte do *pipe*.
    pub(crate) async fn aceitar(&mut self) -> Result<(Conexao, Chamada)> {
        // Espera este cliente, depois já prepara a instância seguinte antes de servir: é o que
        // permite um segundo cliente conectar enquanto o primeiro é atendido.
        self.proximo.connect().await.context("aguardando cliente")?;
        let seguinte = Self::criar(&self.nome, &mut self.seguranca, false)
            .context("preparando o pipe seguinte")?;
        // O descritor de segurança já barrou quem não pode, no próprio sistema.
        Ok((
            std::mem::replace(&mut self.proximo, seguinte),
            Chamada::Permitida,
        ))
    }

    /// Com a autoridade de quem os arquivos pedidos por esta conexão seriam lidos — provisória.
    ///
    /// O descritor de segurança decide **se** alguém entra, mas não diz **quem** entrou, e o
    /// Windows só identifica o cliente depois de o serviço ler algo do *pipe*. Até lá, como SYSTEM,
    /// o serviço não lê nada por ninguém; [`leitor_depois_de_ler`] completa a resposta.
    #[allow(clippy::unused_self)] // a assinatura é a mesma do Linux, onde a escuta tem o que dizer
    pub(crate) fn leitor_de(&self, _conexao: &Conexao) -> ir_transferencia::Leitor {
        ir_transferencia::Leitor::do_chamador(None, 0, crate::lancador::como_servico())
    }
}

/// O leitor desta conexão, agora que o primeiro pedido já foi lido.
///
/// Como SYSTEM, é quem conectou, conferido pelo próprio Windows a cada arquivo aberto
/// (`ir_acesso::identidade`). Rodando como o próprio usuário, não há fronteira a proteger.
#[cfg(windows)]
pub(crate) fn leitor_depois_de_ler(
    conexao: &Conexao,
    _provisorio: ir_transferencia::Leitor,
) -> ir_transferencia::Leitor {
    if !crate::lancador::como_servico() {
        return ir_transferencia::Leitor::Proprio;
    }
    match ir_acesso::identidade::TokenDoCliente::do_pipe(conexao) {
        Ok(token) => ir_transferencia::Leitor::PeloSistema(std::sync::Arc::new(token)),
        Err(erro) => {
            tracing::warn!(%erro, "não consegui identificar quem conectou; esta conexão não envia arquivos");
            ir_transferencia::Leitor::Desconhecido
        }
    }
}

/// No Linux o `SO_PEERCRED` já disse quem conectou, na aceitação.
#[cfg(not(windows))]
pub(crate) const fn leitor_depois_de_ler(
    _conexao: &Conexao,
    da_escuta: ir_transferencia::Leitor,
) -> ir_transferencia::Leitor {
    da_escuta
}

/// O ponto de escuta, que aceita uma conexão de cada vez.
#[cfg(not(windows))]
pub(crate) struct Escuta {
    listener: tokio::net::UnixListener,
    acesso: Acesso,
    /// O usuário que roda o serviço, que sempre pode falar com ele.
    dono: u32,
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
        // O controle fica alcançável por qualquer processo local **de propósito**: quem decide é
        // o serviço, conferindo a credencial de cada conexão em `aceitar`. Deixar o arquivo decidir
        // era deixar a decisão com os grupos congelados do processo que conecta. O canal do
        // agente não tem por que ser alcançável por ninguém além do serviço.
        let modo = match acesso {
            Acesso::Restrito => 0o600,
            Acesso::UsuarioInterativo => 0o666,
        };
        std::fs::set_permissions(caminho, std::fs::Permissions::from_mode(modo))
            .with_context(|| format!("ajustando a permissão de {caminho}"))?;
        Ok(Self {
            listener,
            acesso,
            dono: ir_acesso::grupo::uid_efetivo(),
        })
    }

    /// Espera o próximo cliente e devolve a conexão dele, com a decisão sobre ela.
    ///
    /// # Errors
    ///
    /// Erro de E/S ao aceitar.
    pub(crate) async fn aceitar(&mut self) -> Result<(Conexao, Chamada)> {
        let (conexao, _) = self.listener.accept().await.context("aceitando conexão")?;
        let chamada = self.avaliar(&conexao);
        Ok((conexao, chamada))
    }

    /// Com a autoridade de quem os arquivos pedidos por esta conexão seriam lidos: o `uid` de quem
    /// conectou, pelo `SO_PEERCRED`.
    pub(crate) fn leitor_de(&self, conexao: &Conexao) -> ir_transferencia::Leitor {
        let chamador = conexao.peer_cred().ok().map(|credencial| credencial.uid());
        ir_transferencia::Leitor::do_chamador(chamador, self.dono, true)
    }

    /// Lê quem conectou e pergunta ao porteiro se pode.
    fn avaliar(&self, conexao: &Conexao) -> Chamada {
        let Ok(credencial) = conexao.peer_cred() else {
            // Sem credencial não há como decidir, e na dúvida o canal não abre.
            return Chamada::Negada { uid: u32::MAX };
        };
        let uid = credencial.uid();
        // A filiação é lida agora, no banco de usuários — não a que o processo herdou ao nascer.
        let grupos = ir_acesso::grupo::grupos_do_usuario(uid);
        let chamador = ir_acesso::porteiro::Chamador {
            uid,
            grupos: &grupos,
        };
        ir_acesso::porteiro::decidir(
            self.acesso,
            &chamador,
            self.dono,
            ir_acesso::grupo::gid_do_grupo(ir_acesso::grupo::GRUPO),
        )
    }
}
