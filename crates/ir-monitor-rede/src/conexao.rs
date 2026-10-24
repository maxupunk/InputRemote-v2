//! Um painel conectado: o socket, a conversa, a assinatura.

use std::sync::Arc;
use std::time::Duration;

use ir_monitor::mensagem::{self, DoPainel, ParaPainel};
use ir_monitor::{Apresentacao, Conversa, Evento, Falha};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::{mpsc, watch};
use tokio::time::Instant;

use crate::central::DaConexao;
use crate::{ASSINATURA_EXPIRA, Publicacao};

/// Até quando esperar o aperto de mão terminar. A conexão TCP já está de pé: o que se espera é
/// processamento e um trecho de Wi-Fi.
const PRAZO_DO_APERTO: Duration = Duration::from_secs(10);

/// Até quando esperar a pessoa responder ao código: a janela de pareamento inteira.
const PRAZO_DO_CODIGO: Duration = crate::JANELA_DE_PAREAMENTO;

/// Sem nada do painel por este tempo, ele sumiu: a conexão fecha. Três renovações perdidas
/// além da expiração da assinatura.
const SILENCIO: Duration = Duration::from_secs(15);

/// O que a tarefa recebe da central.
pub(crate) struct Ligacao {
    pub(crate) id: u64,
    pub(crate) nome: Arc<str>,
    pub(crate) central: mpsc::UnboundedSender<DaConexao>,
    pub(crate) decisoes: mpsc::UnboundedReceiver<bool>,
    pub(crate) quadros: watch::Receiver<Option<Arc<Publicacao>>>,
}

/// O estado da conexão, além da conversa.
struct Estado {
    socket: TcpStream,
    conversa: Conversa,
    ligacao: Ligacao,
    /// Até quando a conexão pode ficar sem abrir.
    prazo: Instant,
    ultimo_sinal: Instant,
    assinada_ate: Option<Instant>,
    /// Se o painel quer os detalhes: só um aplicativo da versão 2 ou mais diz.
    quer_detalhes: bool,
    /// A chave do painel, depois da abertura.
    par: Option<ir_crypto::PublicKey>,
}

/// Atende um painel até ele sair, errar ou ser derrubado.
pub(crate) async fn atender(socket: TcpStream, conversa: Conversa, ligacao: Ligacao) {
    let id = ligacao.id;
    let central = ligacao.central.clone();
    let agora = Instant::now();
    let mut estado = Estado {
        socket,
        conversa,
        ligacao,
        prazo: agora + PRAZO_DO_APERTO,
        ultimo_sinal: agora,
        assinada_ate: None,
        quer_detalhes: false,
        par: None,
    };
    if let Err(falha) = estado.laco().await {
        tracing::debug!(%falha, "o painel desconectou");
    }
    let _ = central.send(DaConexao::Fechou(id));
}

/// Por que a conexão acabou.
#[derive(Debug)]
enum Fim {
    Conversa(Falha),
    Socket,
    Prazo,
}

impl core::fmt::Display for Fim {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Conversa(falha) => write!(f, "{falha}"),
            Self::Socket => f.write_str("o socket fechou"),
            Self::Prazo => f.write_str("o painel ficou em silêncio"),
        }
    }
}

impl From<Falha> for Fim {
    fn from(falha: Falha) -> Self {
        Self::Conversa(falha)
    }
}

impl From<std::io::Error> for Fim {
    fn from(_: std::io::Error) -> Self {
        Self::Socket
    }
}

impl Estado {
    async fn laco(&mut self) -> Result<(), Fim> {
        let mut buffer = vec![0u8; 4096];
        let mut batida = tokio::time::interval(Duration::from_secs(1));
        loop {
            tokio::select! {
                lidos = self.socket.read(&mut buffer) => {
                    let lidos = lidos?;
                    let bytes = buffer.get(..lidos).filter(|b| !b.is_empty()).ok_or(Fim::Socket)?;
                    let eventos = self.conversa.alimentar(bytes)?;
                    self.ultimo_sinal = Instant::now();
                    self.tratar(eventos).await?;
                }
                Some(conferiu) = self.ligacao.decisoes.recv() => {
                    let eventos = self.conversa.confirmar(conferiu)?;
                    self.tratar(eventos).await?;
                }
                Ok(()) = self.ligacao.quadros.changed() => self.mandar_quadro().await?,
                _ = batida.tick() => self.vigiar()?,
            }
            if self.conversa.encerrada() {
                return Err(Fim::Conversa(Falha::CodigoRecusado));
            }
        }
    }

    async fn tratar(&mut self, eventos: Vec<Evento>) -> Result<(), Fim> {
        for evento in eventos {
            match evento {
                Evento::Enviar(bytes) => self.socket.write_all(&bytes).await?,
                Evento::Codigo { digitos, .. } => {
                    self.prazo = Instant::now() + PRAZO_DO_CODIGO;
                    self.contar(DaConexao::Codigo(self.ligacao.id, digitos));
                }
                Evento::Aberta { par, pareou } => {
                    self.par = Some(par);
                    let id = self.ligacao.id;
                    self.contar(DaConexao::Aberta { id, par, pareou });
                }
                Evento::Mensagem(conteudo) => self.mensagem(&conteudo).await?,
            }
        }
        Ok(())
    }

    async fn mensagem(&mut self, conteudo: &[u8]) -> Result<(), Fim> {
        match mensagem::decodificar::<DoPainel>(conteudo).map_err(|_| Falha::Malformado)? {
            DoPainel::Ola(apresentacao) => {
                if let Some(par) = self.par {
                    self.contar(DaConexao::Apresentou(par, apresentacao.nome));
                }
                let resposta = ParaPainel::Ola(Apresentacao::desta_ponta(&self.ligacao.nome));
                let bytes = mensagem::codificar(&resposta).map_err(|_| Falha::Malformado)?;
                let cifrado = self.conversa.enviar(&bytes)?;
                self.socket.write_all(&cifrado).await?;
            }
            DoPainel::Assinar(quer) => {
                let antes = self.assinada_ate.is_some();
                self.assinada_ate = quer.then(|| Instant::now() + ASSINATURA_EXPIRA);
                if quer != antes {
                    self.contar(DaConexao::Assinatura(self.ligacao.id, quer));
                }
                if quer && !antes {
                    // Quem acabou de assinar vê o último quadro já, e não daqui a um segundo.
                    self.mandar_quadro().await?;
                }
            }
            DoPainel::Interesse(interesse) => {
                self.quer_detalhes = true;
                self.contar(DaConexao::Interesse(self.ligacao.id, interesse));
                // A tela que acabou de pedir — a lista aberta agora — vê algo já, e não daqui a
                // um segundo.
                self.mandar_quadro().await?;
            }
        }
        Ok(())
    }

    async fn mandar_quadro(&mut self) -> Result<(), Fim> {
        if self.assinada_ate.is_none() || !self.conversa.aberta() {
            return Ok(());
        }
        let Some(publicacao) = self.ligacao.quadros.borrow_and_update().clone() else {
            return Ok(());
        };
        let detalhes = publicacao.detalhes.as_ref().filter(|_| self.quer_detalhes);
        for bytes in std::iter::once(&publicacao.quadro).chain(detalhes) {
            let cifrado = self.conversa.enviar(bytes)?;
            self.socket.write_all(&cifrado).await?;
        }
        Ok(())
    }

    /// A cada segundo: o prazo de abrir, a assinatura que expirou, o painel que sumiu.
    fn vigiar(&mut self) -> Result<(), Fim> {
        let agora = Instant::now();
        if !self.conversa.aberta() && agora >= self.prazo {
            return Err(Fim::Prazo);
        }
        if self.assinada_ate.is_some_and(|ate| agora >= ate) {
            self.assinada_ate = None;
            self.contar(DaConexao::Assinatura(self.ligacao.id, false));
        }
        if self.conversa.aberta() && agora.duration_since(self.ultimo_sinal) >= SILENCIO {
            return Err(Fim::Prazo);
        }
        Ok(())
    }

    fn contar(&self, noticia: DaConexao) {
        let _ = self.ligacao.central.send(noticia);
    }
}
