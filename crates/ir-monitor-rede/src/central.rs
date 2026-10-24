//! A dona do estado do monitor: quem está conectado, quem pareia, quem assina.

use std::collections::HashMap;
use std::net::{Ipv4Addr, SocketAddr};
use std::sync::Arc;

use ir_crypto::{Identity, PublicKey};
use ir_monitor::{Conversa, Interesse, InteresseEmProcessos, Politica};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{mpsc, watch};
use tokio::task::JoinHandle;
use tokio::time::Instant;

use crate::conexao::{self, Ligacao};
use crate::{Aviso, Comando, JANELA_DE_PAREAMENTO, Publicacao};

/// O que uma conexão conta à central.
#[derive(Debug)]
pub(crate) enum DaConexao {
    /// Um socket novo, da escuta.
    Nova(TcpStream),
    /// A conexão chegou ao código de pareamento.
    Codigo(u64, [u8; 6]),
    /// A conversa abriu.
    Aberta {
        /// Qual conexão.
        id: u64,
        /// A chave do painel.
        par: PublicKey,
        /// Se abriu por pareamento agora.
        pareou: bool,
    },
    /// O painel disse o nome.
    Apresentou(PublicKey, String),
    /// O painel começou ou parou de assinar.
    Assinatura(u64, bool),
    /// O painel disse o que quer além do quadro.
    Interesse(u64, Interesse),
    /// A conexão acabou.
    Fechou(u64),
}

/// O que a central precisa para nascer.
pub(crate) struct Partes {
    pub(crate) identidade: Identity,
    pub(crate) nome: Arc<str>,
    pub(crate) conhecidos: Vec<PublicKey>,
    pub(crate) avisos: mpsc::UnboundedSender<Aviso>,
    pub(crate) quadros: watch::Receiver<Option<Arc<Publicacao>>>,
    pub(crate) porta: u16,
}

/// Uma conexão viva, vista da central.
struct Viva {
    decisoes: mpsc::UnboundedSender<bool>,
    par: Option<PublicKey>,
    assinando: bool,
    interesse: Option<Interesse>,
    tarefa: JoinHandle<()>,
}

pub(crate) struct Central {
    partes: Partes,
    aberto_ate: Option<Instant>,
    pareando: Option<u64>,
    vivas: HashMap<u64, Viva>,
    proximo: u64,
    escuta: Option<JoinHandle<()>>,
    da_conexao: mpsc::UnboundedSender<DaConexao>,
    recebidas: mpsc::UnboundedReceiver<DaConexao>,
    assinantes: usize,
    interesse: [InteresseEmProcessos; 2],
}

impl Central {
    pub(crate) fn nova(partes: Partes) -> Self {
        let (da_conexao, recebidas) = mpsc::unbounded_channel();
        Self {
            partes,
            aberto_ate: None,
            pareando: None,
            vivas: HashMap::new(),
            proximo: 0,
            escuta: None,
            da_conexao,
            recebidas,
            assinantes: 0,
            interesse: Default::default(),
        }
    }

    /// O laço da central, até o serviço soltar o [`Monitor`](crate::Monitor).
    pub(crate) async fn rodar(mut self, mut comandos: mpsc::UnboundedReceiver<Comando>) {
        self.ajustar_escuta().await;
        loop {
            let prazo = self.aberto_ate;
            tokio::select! {
                comando = comandos.recv() => match comando {
                    Some(comando) => self.comando(comando).await,
                    None => break,
                },
                Some(noticia) = self.recebidas.recv() => self.noticia(noticia).await,
                () = dormir_ate(prazo) => self.fechar_pareamento().await,
            }
        }
        self.encerrar();
    }

    async fn comando(&mut self, comando: Comando) {
        match comando {
            Comando::AbrirPareamento => {
                self.aberto_ate = Some(Instant::now() + JANELA_DE_PAREAMENTO);
                self.ajustar_escuta().await;
            }
            Comando::FecharPareamento => self.fechar_pareamento().await,
            Comando::Confirmar(conferiu) => {
                let decisoes = self.pareando.and_then(|id| self.vivas.get(&id));
                if let Some(viva) = decisoes {
                    let _ = viva.decisoes.send(conferiu);
                }
            }
            Comando::Conhecidos(chaves) => {
                self.partes.conhecidos = chaves;
                self.derrubar_desconhecidos();
                self.ajustar_escuta().await;
            }
        }
    }

    async fn noticia(&mut self, noticia: DaConexao) {
        match noticia {
            DaConexao::Nova(socket) => self.aceitar(socket),
            DaConexao::Codigo(id, digitos) => self.codigo(id, digitos),
            DaConexao::Aberta { id, par, pareou } => self.aberta(id, par, pareou).await,
            DaConexao::Apresentou(chave, nome) => self.avisar(Aviso::Apresentou { chave, nome }),
            DaConexao::Assinatura(id, assinando) => {
                if let Some(viva) = self.vivas.get_mut(&id) {
                    viva.assinando = assinando;
                }
                self.contar_assinantes();
            }
            DaConexao::Interesse(id, interesse) => {
                if let Some(viva) = self.vivas.get_mut(&id) {
                    viva.interesse = Some(interesse);
                }
                self.contar_assinantes();
            }
            DaConexao::Fechou(id) => self.fechou(id),
        }
    }

    fn aceitar(&mut self, socket: TcpStream) {
        let politica = Politica {
            conhecidos: self.partes.conhecidos.clone(),
            // Um pareamento de cada vez: com um em curso, o próximo painel novo espera a vez.
            aceita_pareamento: self.janela_aberta() && self.pareando.is_none(),
        };
        let conversa = Conversa::atender(self.partes.identidade.clone(), politica);
        let id = self.proximo;
        self.proximo += 1;
        let (decisoes, recebe_decisoes) = mpsc::unbounded_channel();
        let ligacao = Ligacao {
            id,
            nome: Arc::clone(&self.partes.nome),
            central: self.da_conexao.clone(),
            decisoes: recebe_decisoes,
            quadros: self.partes.quadros.clone(),
        };
        let tarefa = tokio::spawn(conexao::atender(socket, conversa, ligacao));
        let viva = Viva {
            decisoes,
            par: None,
            assinando: false,
            interesse: None,
            tarefa,
        };
        self.vivas.insert(id, viva);
    }

    fn codigo(&mut self, id: u64, digitos: [u8; 6]) {
        if self.pareando.is_some() || !self.janela_aberta() {
            // Outro painel chegou primeiro, ou a janela fechou no meio do aperto de mão.
            if let Some(viva) = self.vivas.get(&id) {
                let _ = viva.decisoes.send(false);
            }
            return;
        }
        self.pareando = Some(id);
        self.avisar(Aviso::Codigo(digitos));
    }

    async fn aberta(&mut self, id: u64, par: PublicKey, pareou: bool) {
        if let Some(viva) = self.vivas.get_mut(&id) {
            viva.par = Some(par);
        }
        if pareou {
            self.pareando = None;
            self.aberto_ate = None;
            if !self.partes.conhecidos.contains(&par) {
                self.partes.conhecidos.push(par);
            }
            self.avisar(Aviso::Pareado(par));
            self.ajustar_escuta().await;
        }
        self.contar_conectados();
    }

    fn fechou(&mut self, id: u64) {
        self.vivas.remove(&id);
        if self.pareando == Some(id) {
            self.pareando = None;
            self.avisar(Aviso::PareamentoFalhou);
        }
        self.contar_assinantes();
        self.contar_conectados();
    }

    async fn fechar_pareamento(&mut self) {
        self.aberto_ate = None;
        if let Some(id) = self.pareando.take() {
            if let Some(viva) = self.vivas.get(&id) {
                let _ = viva.decisoes.send(false);
            }
            self.avisar(Aviso::PareamentoFalhou);
        }
        self.ajustar_escuta().await;
    }

    /// Quem saiu da lista de pareados sai também da conexão, na hora.
    fn derrubar_desconhecidos(&mut self) {
        let conhecidos = &self.partes.conhecidos;
        self.vivas.retain(|_, viva| {
            let fica = viva.par.is_none_or(|par| conhecidos.contains(&par));
            if !fica {
                viva.tarefa.abort();
            }
            fica
        });
        self.contar_assinantes();
        self.contar_conectados();
    }

    fn janela_aberta(&self) -> bool {
        self.aberto_ate.is_some_and(|ate| Instant::now() < ate)
    }

    /// A porta aberta só quando há a quem atender: um painel pareado, ou a janela aberta.
    async fn ajustar_escuta(&mut self) {
        let precisa = !self.partes.conhecidos.is_empty() || self.janela_aberta();
        match (precisa, self.escuta.is_some()) {
            (true, false) => self.escuta = escutar(self.partes.porta, &self.da_conexao).await,
            (false, true) => {
                if let Some(escuta) = self.escuta.take() {
                    escuta.abort();
                }
            }
            _ => {}
        }
    }

    /// Quantos assinam e o que querem dos processos — o que liga e desliga a coleta.
    fn contar_assinantes(&mut self) {
        let agora = self.vivas.values().filter(|v| v.assinando).count();
        if agora != self.assinantes {
            self.assinantes = agora;
            self.avisar(Aviso::Assinantes(agora));
        }
        let interesse = Interesse::por_maquina(
            self.vivas
                .values()
                .filter(|v| v.assinando)
                .filter_map(|v| v.interesse.as_ref()),
        );
        if interesse != self.interesse {
            self.avisar(Aviso::Interesse(interesse.clone()));
            self.interesse = interesse;
        }
    }

    fn contar_conectados(&self) {
        let chaves = self.vivas.values().filter_map(|v| v.par).collect();
        self.avisar(Aviso::Conectados(chaves));
    }

    fn avisar(&self, aviso: Aviso) {
        let _ = self.partes.avisos.send(aviso);
    }

    fn encerrar(&mut self) {
        if let Some(escuta) = self.escuta.take() {
            escuta.abort();
        }
        for viva in self.vivas.values() {
            viva.tarefa.abort();
        }
    }
}

/// Abre a porta e passa cada socket aceito à central.
async fn escutar(porta: u16, central: &mpsc::UnboundedSender<DaConexao>) -> Option<JoinHandle<()>> {
    let endereco = SocketAddr::from((Ipv4Addr::UNSPECIFIED, porta));
    let escuta = match TcpListener::bind(endereco).await {
        Ok(escuta) => escuta,
        Err(erro) => {
            tracing::warn!(%erro, porta, "a porta do painel não abriu; o painel não vai conectar");
            return None;
        }
    };
    tracing::info!(porta, "atendendo o painel");
    let central = central.clone();
    Some(tokio::spawn(async move {
        while let Ok((socket, _)) = escuta.accept().await {
            let _ = socket.set_nodelay(true);
            if central.send(DaConexao::Nova(socket)).is_err() {
                break;
            }
        }
    }))
}

/// Espera até o prazo, ou para sempre sem prazo.
async fn dormir_ate(prazo: Option<Instant>) {
    match prazo {
        Some(prazo) => tokio::time::sleep_until(prazo).await,
        None => std::future::pending().await,
    }
}
