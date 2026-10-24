//! A ligação com um computador, numa linha própria: discar, conferir o código, receber o quadro.
//!
//! O aplicativo manda [`Comando`]s e recebe [`Evento`]s por canal; a linha é dona do socket e da
//! [`Conversa`]. O socket lê com prazo curto para a linha poder, entre uma leitura e outra, atender
//! um comando e renovar a assinatura — o computador a esquece em 5 s sem renovação.

use std::io::{ErrorKind, Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, Sender, TryRecvError};
use std::time::{Duration, Instant};

use ir_crypto::enlace::ConnectMode;
use ir_monitor::mensagem::{self, DoPainel, ParaPainel};
use ir_monitor::{Apresentacao, Conversa, Evento as DaConversa, Vista};

use crate::cofre::{Cofre, Computador};
use crate::descoberta;

/// Quanto esperar o TCP abrir.
const PRAZO_DE_CONEXAO: Duration = Duration::from_secs(3);
/// O prazo de cada leitura: o passo da linha.
const PASSO: Duration = Duration::from_millis(250);
/// A cada quanto a assinatura é renovada. O computador a esquece em 5 s.
const RENOVAR: Duration = Duration::from_secs(2);
/// Sem nada do computador por este tempo, aberta a conversa, ele sumiu. O quadro vem a cada 1 s.
const SILENCIO: Duration = Duration::from_secs(6);
/// Quanto a descoberta procura quando o computador pareado não está no último IP.
const PROCURA: Duration = Duration::from_millis(1500);

/// Com quem ligar.
#[derive(Debug, Clone)]
pub enum Destino {
    /// Um computador novo, achado na rede: pareia do zero, com o código.
    Parear {
        /// Como ele se anunciou.
        nome: String,
        /// Onde o painel atende nele: o IP de onde respondeu, na [`ir_monitor::PORTA`].
        endereco: SocketAddr,
    },
    /// Um computador já pareado: volta com a chave dele fixada.
    Voltar(Computador),
}

/// O que o aplicativo pede à linha.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Comando {
    /// A resposta desta tela ao código.
    Confirmar(bool),
    /// Começar (`true`) ou parar de receber o quadro.
    Assinar(bool),
    /// Encerrar a ligação.
    Fechar,
}

/// O que a linha conta ao aplicativo.
#[derive(Debug, Clone, PartialEq)]
pub enum Evento {
    /// Pareando: mostre o código e pergunte.
    Codigo([u8; 6]),
    /// A ligação abriu com este computador.
    Aberta {
        /// O nome dele.
        computador: String,
        /// Se foi um pareamento agora.
        pareou: bool,
    },
    /// O quadro deste segundo, pronto para desenhar.
    Quadro(Vista),
    /// A ligação acabou.
    Caiu {
        /// Por quê, em uma frase.
        motivo: String,
        /// Se o computador recusou este aparelho — não adianta tentar de novo sem parear.
        recusado: bool,
    },
}

/// O que a linha precisa para rodar.
#[derive(Debug)]
pub struct Partes {
    /// Onde está o cofre.
    pub pasta: PathBuf,
    /// O nome deste aparelho, para a lista do computador.
    pub nome_do_aparelho: String,
    /// Com quem ligar.
    pub destino: Destino,
    /// Por onde chegam os comandos.
    pub comandos: Receiver<Comando>,
    /// Por onde saem os eventos.
    pub eventos: Sender<Evento>,
}

/// Roda a ligação até ela acabar. Termina sempre com um [`Evento::Caiu`].
pub fn rodar(partes: Partes) {
    let eventos = partes.eventos.clone();
    let (motivo, recusado) = match Ligacao::abrir(partes) {
        Ok(mut ligacao) => ligacao.laco(),
        Err(motivo) => (motivo, false),
    };
    let _ = eventos.send(Evento::Caiu { motivo, recusado });
}

struct Ligacao {
    socket: TcpStream,
    conversa: Conversa,
    cofre: Cofre,
    computador: Computador,
    pareando: bool,
    nome_do_aparelho: String,
    comandos: Receiver<Comando>,
    eventos: Sender<Evento>,
    assinando: bool,
    renovada: Instant,
    ultimo_sinal: Instant,
}

impl Ligacao {
    fn abrir(partes: Partes) -> Result<Self, String> {
        let mut cofre = Cofre::abrir(&partes.pasta).map_err(|e| e.to_string())?;
        let (computador, modo, socket) = discar(&mut cofre, partes.destino)?;
        let (conversa, primeiros) =
            Conversa::discar(cofre.identidade(), modo).map_err(|e| e.to_string())?;
        let mut socket = socket;
        socket.write_all(&primeiros).map_err(|e| e.to_string())?;
        let agora = Instant::now();
        Ok(Self {
            socket,
            conversa,
            cofre,
            pareando: matches!(modo, ConnectMode::Pair),
            computador,
            nome_do_aparelho: partes.nome_do_aparelho,
            comandos: partes.comandos,
            eventos: partes.eventos,
            assinando: false,
            renovada: agora,
            ultimo_sinal: agora,
        })
    }

    /// O laço da ligação. Devolve por que acabou, e se foi recusa.
    fn laco(&mut self) -> (String, bool) {
        let mut buffer = [0u8; 4096];
        loop {
            if let Err(fim) = self.atender_comandos().and_then(|()| self.renovar()) {
                return fim;
            }
            match self.socket.read(&mut buffer) {
                Ok(0) => return ("o computador fechou a ligação".to_owned(), false),
                Ok(lidos) => {
                    let bytes = buffer.get(..lidos).unwrap_or_default();
                    if let Err(fim) = self.receber(bytes) {
                        return fim;
                    }
                }
                Err(e) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {}
                Err(e) => return (format!("a rede caiu: {e}"), false),
            }
            if self.conversa.aberta() && self.ultimo_sinal.elapsed() >= SILENCIO {
                return ("o computador parou de responder".to_owned(), false);
            }
        }
    }

    fn atender_comandos(&mut self) -> Result<(), (String, bool)> {
        loop {
            match self.comandos.try_recv() {
                Ok(Comando::Fechar) | Err(TryRecvError::Disconnected) => {
                    return Err(("ligação encerrada".to_owned(), false));
                }
                Err(TryRecvError::Empty) => return Ok(()),
                Ok(Comando::Confirmar(conferiu)) => {
                    let eventos = self.conversa.confirmar(conferiu).map_err(falha)?;
                    self.tratar(eventos)?;
                    if self.conversa.encerrada() {
                        return Err(("o código não foi confirmado".to_owned(), false));
                    }
                }
                Ok(Comando::Assinar(quer)) => {
                    self.assinando = quer;
                    if self.conversa.aberta() {
                        self.mandar(&DoPainel::Assinar(quer))?;
                    }
                }
            }
        }
    }

    fn renovar(&mut self) -> Result<(), (String, bool)> {
        if self.assinando && self.conversa.aberta() && self.renovada.elapsed() >= RENOVAR {
            self.mandar(&DoPainel::Assinar(true))?;
        }
        Ok(())
    }

    fn receber(&mut self, bytes: &[u8]) -> Result<(), (String, bool)> {
        self.ultimo_sinal = Instant::now();
        let eventos = self.conversa.alimentar(bytes).map_err(falha)?;
        self.tratar(eventos)
    }

    fn tratar(&mut self, eventos: Vec<DaConversa>) -> Result<(), (String, bool)> {
        for evento in eventos {
            match evento {
                DaConversa::Enviar(bytes) => self.escrever(&bytes)?,
                DaConversa::Codigo { digitos, .. } => self.contar(Evento::Codigo(digitos)),
                DaConversa::Aberta { par, pareou } => self.abriu(par.hex(), pareou)?,
                DaConversa::Mensagem(conteudo) => self.mensagem(&conteudo),
            }
        }
        Ok(())
    }

    fn abriu(&mut self, chave: String, pareou: bool) -> Result<(), (String, bool)> {
        self.computador.chave = chave;
        // Pareou agora, ou voltou por um IP novo: o cofre guarda o que vale daqui em diante.
        let _ = self.cofre.guardar(self.computador.clone());
        self.mandar(&DoPainel::Ola(Apresentacao::desta_ponta(
            &self.nome_do_aparelho,
        )))?;
        if self.assinando {
            self.mandar(&DoPainel::Assinar(true))?;
        }
        self.contar(Evento::Aberta {
            computador: self.computador.nome.clone(),
            pareou: pareou && self.pareando,
        });
        Ok(())
    }

    fn mensagem(&mut self, conteudo: &[u8]) {
        match mensagem::decodificar::<ParaPainel>(conteudo) {
            Ok(ParaPainel::Quadro(quadro)) => {
                self.contar(Evento::Quadro(Vista::do_quadro(&quadro)));
            }
            Ok(ParaPainel::Ola(apresentacao)) if apresentacao.nome != self.computador.nome => {
                self.computador.nome = apresentacao.nome;
                let _ = self.cofre.guardar(self.computador.clone());
            }
            // Uma mensagem de uma versão mais nova do computador: o painel segue com o que entende.
            Ok(ParaPainel::Ola(_)) | Err(_) => {}
        }
    }

    fn mandar(&mut self, mensagem: &DoPainel) -> Result<(), (String, bool)> {
        let claro = mensagem::codificar(mensagem).map_err(|e| (e.to_string(), false))?;
        let cifrado = self.conversa.enviar(&claro).map_err(falha)?;
        if matches!(mensagem, DoPainel::Assinar(true)) {
            self.renovada = Instant::now();
        }
        self.escrever(&cifrado)
    }

    fn escrever(&mut self, bytes: &[u8]) -> Result<(), (String, bool)> {
        self.socket
            .write_all(bytes)
            .map_err(|e| (format!("a rede caiu: {e}"), false))
    }

    fn contar(&self, evento: Evento) {
        let _ = self.eventos.send(evento);
    }
}

fn falha(falha: ir_monitor::Falha) -> (String, bool) {
    let recusado = matches!(falha, ir_monitor::Falha::Recusado);
    (falha.to_string(), recusado)
}

/// Abre o TCP com o destino e escolhe o modo do aperto de mão.
fn discar(
    cofre: &mut Cofre,
    destino: Destino,
) -> Result<(Computador, ConnectMode, TcpStream), String> {
    match destino {
        Destino::Parear { nome, endereco } => {
            let socket = conectar(endereco)?;
            let computador = Computador {
                nome,
                chave: String::new(),
                endereco: endereco.to_string(),
            };
            Ok((computador, ConnectMode::Pair, socket))
        }
        Destino::Voltar(mut computador) => {
            let chave = computador
                .chave()
                .ok_or("a chave gravada deste computador está ilegível")?;
            let socket = match conectar_no_ultimo(&computador) {
                Some(socket) => socket,
                None => reencontrar(cofre, &mut computador)?,
            };
            Ok((computador, ConnectMode::Reconnect(chave), socket))
        }
    }
}

fn conectar_no_ultimo(computador: &Computador) -> Option<TcpStream> {
    conectar(computador.endereco_do_painel()?).ok()
}

/// O computador não está no último IP: pergunta à rede quem tem a mesma instalação — a chave fixa
/// quem ele é, e o DHCP pode ter mudado onde ele está.
fn reencontrar(cofre: &mut Cofre, computador: &mut Computador) -> Result<TcpStream, String> {
    let maquina = computador.maquina().unwrap_or_default();
    let achado = descoberta::procurar(PROCURA)
        .into_iter()
        .find(|achado| achado.maquina == maquina)
        .ok_or_else(|| format!("{} não está na rede", computador.nome))?;
    let endereco = descoberta::do_painel(achado.endereco);
    let socket = conectar(endereco)?;
    computador.endereco = endereco.to_string();
    let _ = cofre.guardar(computador.clone());
    Ok(socket)
}

fn conectar(endereco: SocketAddr) -> Result<TcpStream, String> {
    let socket = TcpStream::connect_timeout(&endereco, PRAZO_DE_CONEXAO)
        .map_err(|e| format!("o computador não atendeu em {endereco}: {e}"))?;
    let _ = socket.set_nodelay(true);
    socket
        .set_read_timeout(Some(PASSO))
        .map_err(|e| e.to_string())?;
    Ok(socket)
}
