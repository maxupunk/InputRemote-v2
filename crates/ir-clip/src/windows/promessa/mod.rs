//! A promessa de arquivos no clipboard: colar antes de a cópia chegar.
//!
//! Do Linux, a cópia de arquivos só começa quando o mouse chega ao Windows — o GNOME não avisa a
//! mudança do clipboard, e a travessia é o gatilho ([ADR-0011](../../../../../docs/adr/0011-clipboard-na-travessia.md)).
//! Quem atravessa para colar cola logo, e antes disto colava o que o clipboard tinha **antes**.
//!
//! Agora, assim que uma cópia começa a chegar, o clipboard recebe os arquivos como **virtuais**
//! ([`dados`]): o Explorer cola na hora, abre o próprio diálogo de cópia, e lê cada arquivo à medida
//! que ele chega ([`fluxo`]). Quando a cópia termina, os arquivos de verdade tomam o lugar deles —
//! para os programas que só entendem a lista de caminhos (`CF_HDROP`).
//!
//! # As regras, que são de produto
//!
//! - **A última cópia vale.** Se a pessoa copiar outra coisa enquanto os arquivos chegam, eles não
//!   passam por cima da cópia nova ([`Promessa::cumprir`]).
//! - **A cópia que não chega desfaz a promessa**: quem estava colando recebe erro no diálogo do
//!   Explorer, e o clipboard deixa de prometer. Uma promessa nova toma o lugar da anterior.
//! - **Ninguém daqui lê a promessa** ([`e_dona`]): ela é do próprio ajudante.
//!
//! # A thread
//!
//! O OLE exige que o objeto do clipboard pertença a uma thread com apartamento de thread única e
//! fila de mensagens — é por ela que o sistema conversa com o objeto. A thread é dedicada; o resto
//! do ajudante só manda comandos e espera a resposta, com prazo.

#![allow(unsafe_code)]

mod andamento;
mod dados;
mod fluxo;

use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::Duration;

use windows::Win32::Foundation::{LPARAM, WPARAM};
use windows::Win32::System::Com::IDataObject;
use windows::Win32::System::DataExchange::GetClipboardOwner;
use windows::Win32::System::Ole::{OleInitialize, OleSetClipboard, OleUninitialize};
use windows::Win32::System::Threading::{GetCurrentProcessId, GetCurrentThreadId};
use windows::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, GetMessageW, GetWindowThreadProcessId, MSG, PM_NOREMOVE, PeekMessageW,
    PostThreadMessageW, TranslateMessage, WM_APP,
};

use self::andamento::Andamento;
use self::dados::{DadosVirtuais, Formatos};
use crate::chegada::Chegada;
use crate::error::{ClipError, Result};

/// Quanto quem promete espera a thread dona atender.
const PRAZO_DA_DONA: Duration = Duration::from_secs(2);

/// Quantas vezes tentar pôr o objeto no clipboard ocupado, e quanto esperar entre elas: o mesmo
/// meio segundo de toda abertura do clipboard ([`super::repetir`]).
const TENTATIVAS: u32 = 10;
const ESPERA: Duration = Duration::from_millis(50);

/// Acorda a thread dona para ler os comandos.
const COMANDO: u32 = WM_APP + 1;

/// Como a promessa terminou para quem ia publicar os arquivos.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Cumprimento {
    /// Não havia promessa, ou ela ainda está no clipboard: publicar os arquivos de verdade.
    Publicar,
    /// A pessoa copiou outra coisa enquanto os arquivos chegavam: não passar por cima.
    CopiaMaisNova,
}

/// O que a thread dona faz.
enum Comando {
    /// Põe estes arquivos virtuais no clipboard, e responde se deu.
    Prometer(Arc<Andamento>, Sender<Result<()>>),
    /// Tira a promessa do clipboard, se ela ainda estiver lá.
    Desfazer,
}

/// A promessa em curso, e a thread que a sustenta.
#[derive(Debug)]
pub(super) struct Promessa {
    comandos: Sender<Comando>,
    thread: u32,
    atual: Option<Arc<Andamento>>,
}

impl std::fmt::Debug for Comando {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Prometer(..) => "Prometer",
            Self::Desfazer => "Desfazer",
        })
    }
}

impl Promessa {
    /// Sobe a thread dona.
    ///
    /// # Errors
    ///
    /// [`ClipError::Indisponivel`] se o OLE não iniciar — fora de uma sessão de usuário.
    pub(super) fn nova() -> Result<Self> {
        let (comandos, recebe) = mpsc::channel();
        let (pronta, espera) = mpsc::channel();
        thread::spawn(move || dona(&recebe, &pronta));
        let thread = espera
            .recv()
            .map_err(|_| ClipError::Indisponivel("a thread do clipboard não subiu"))??;
        Ok(Self {
            comandos,
            thread,
            atual: None,
        })
    }

    fn mandar(&self, comando: Comando) -> bool {
        self.comandos.send(comando).is_ok()
            && unsafe { PostThreadMessageW(self.thread, COMANDO, WPARAM(0), LPARAM(0)) }.is_ok()
    }

    /// Promete os arquivos desta chegada, no lugar de qualquer promessa anterior.
    ///
    /// # Errors
    ///
    /// [`ClipError::FormatoNaoSuportado`] se algum caminho não couber num descritor;
    /// [`ClipError::Ocupado`] se o clipboard não abriu a tempo.
    pub(super) fn prometer(&mut self, chegada: &Chegada) -> Result<()> {
        dados::descritor(&chegada.itens)?;
        if let Some(anterior) = self.atual.take() {
            anterior.falhar();
        }
        let andamento = Arc::new(Andamento::novo(chegada.clone()));
        let (resposta, recebe) = mpsc::channel();
        if !self.mandar(Comando::Prometer(Arc::clone(&andamento), resposta)) {
            return Err(ClipError::Indisponivel("a thread do clipboard saiu"));
        }
        match recebe.recv_timeout(PRAZO_DA_DONA) {
            Ok(Ok(())) => {
                self.atual = Some(andamento);
                Ok(())
            }
            Ok(Err(erro)) => Err(erro),
            Err(_) => Err(ClipError::Ocupado),
        }
    }

    /// A cópia chegou e foi publicada: quem está colando passa a ler de onde ela ficou.
    pub(super) fn cumprir(&mut self) -> Cumprimento {
        let Some(andamento) = self.atual.take() else {
            return Cumprimento::Publicar;
        };
        andamento.publicar();
        if e_dona() {
            Cumprimento::Publicar
        } else {
            Cumprimento::CopiaMaisNova
        }
    }

    /// A cópia não chega mais.
    pub(super) fn desfazer(&mut self) {
        if let Some(andamento) = self.atual.take() {
            andamento.falhar();
            self.mandar(Comando::Desfazer);
        }
    }
}

/// Se o clipboard agora é deste processo — a promessa ainda está lá.
///
/// O OLE põe o objeto no clipboard em nome de uma janela dele, na thread dona; a publicação comum
/// ([`super::area::publicar`]) não deixa dono nenhum. Então "a dona é uma janela deste processo" é
/// "a promessa está lá".
pub(super) fn e_dona() -> bool {
    let Ok(dona) = (unsafe { GetClipboardOwner() }) else {
        return false;
    };
    if dona.is_invalid() {
        return false;
    }
    let mut processo = 0u32;
    unsafe { GetWindowThreadProcessId(dona, Some(&raw mut processo)) };
    processo == unsafe { GetCurrentProcessId() }
}

/// A thread dona: inicia o OLE, diz que está pronta, e atende comandos e sistema até o fim.
fn dona(comandos: &Receiver<Comando>, pronta: &Sender<Result<u32>>) {
    if unsafe { OleInitialize(None) }.is_err() {
        let _ = pronta.send(Err(ClipError::Indisponivel("o OLE não iniciou")));
        return;
    }
    let formatos = match Formatos::registrar() {
        Ok(formatos) => formatos,
        Err(erro) => {
            let _ = pronta.send(Err(erro));
            unsafe { OleUninitialize() };
            return;
        }
    };
    let mut mensagem = MSG::default();
    // Cria a fila de mensagens da thread antes de dizer que está pronta: um comando postado antes
    // dela existir se perderia.
    let _ = unsafe { PeekMessageW(&raw mut mensagem, None, 0, 0, PM_NOREMOVE) };
    let _ = pronta.send(Ok(unsafe { GetCurrentThreadId() }));

    let mut atual: Option<IDataObject> = None;
    while unsafe { GetMessageW(&raw mut mensagem, None, 0, 0) }.0 > 0 {
        if mensagem.hwnd.is_invalid() && mensagem.message == COMANDO {
            while let Ok(comando) = comandos.try_recv() {
                atender(comando, &mut atual, formatos);
            }
            continue;
        }
        unsafe {
            let _ = TranslateMessage(&raw const mensagem);
            DispatchMessageW(&raw const mensagem);
        }
    }
    drop(atual);
    unsafe { OleUninitialize() };
}

fn atender(comando: Comando, atual: &mut Option<IDataObject>, formatos: Formatos) {
    match comando {
        Comando::Prometer(andamento, resposta) => {
            let objeto: IDataObject = DadosVirtuais::novo(andamento, formatos).into();
            let resultado = por_no_clipboard(&objeto);
            if resultado.is_ok() {
                *atual = Some(objeto);
            }
            let _ = resposta.send(resultado);
        }
        Comando::Desfazer => {
            if atual.take().is_some() && e_dona() {
                let _ = unsafe { OleSetClipboard(None) };
            }
        }
    }
}

/// Põe o objeto no clipboard, com a repetição curta de toda abertura dele.
fn por_no_clipboard(objeto: &IDataObject) -> Result<()> {
    for _ in 0..TENTATIVAS {
        if unsafe { OleSetClipboard(objeto) }.is_ok() {
            return Ok(());
        }
        thread::sleep(ESPERA);
    }
    Err(ClipError::Ocupado)
}
