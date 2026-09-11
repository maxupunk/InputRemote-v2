//! O serviço de verdade, falado pelo canal de controle de IPC.
//!
//! Esta é a implementação de [`Servico`](crate::servico::Servico) que substitui o simulado: em
//! vez de responder de memória, ela conversa com o `inputremote-daemon` por um *named pipe* (no
//! Windows) ou socket Unix (no Linux). A interface não muda em nada — ela fala com um `dyn
//! Servico`, e trocar qual é a única linha que difere, em `main`.
//!
//! # Como o duplex vira pedido-resposta mais avisos
//!
//! O canal carrega dois tipos de mensagem misturados: a resposta a um pedido e um aviso que o
//! serviço manda por conta própria ([`ir_ipc::ParaInterface`]). Uma thread de leitura separa os
//! dois: respostas vão para um canal que [`ServicoReal::pedir`] espera; avisos entram numa fila
//! que [`ServicoReal::avisos`] esvazia. Como a interface roda numa thread só e o Slint não é
//! `tokio`, tudo aqui é bloqueante e de biblioteca padrão — sem runtime assíncrono.

use std::collections::VecDeque;
use std::io::{Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use ir_ipc::codec::{self, PREFIXO};
use ir_ipc::{Autoridade, Aviso, ParaInterface, Pedido, Resposta};

use crate::servico::Servico;

/// Quanto tempo um pedido espera pela resposta antes de desistir.
///
/// Todo pedido é local e curto; um que passe disto é sinal de serviço travado, e travar a janela
/// junto seria a pior resposta possível.
const ESPERA: Duration = Duration::from_secs(2);

/// O canal de escrita e o de respostas, juntos sob um cadeado só.
///
/// Ficam juntos para serializar a chamada: um pedido escreve e então espera a resposta dele, sem
/// que outro pedido se enfie no meio e receba a resposta trocada.
struct Canal {
    escrita: Box<dyn Write + Send>,
    respostas: Receiver<Resposta>,
}

/// O serviço falado pelo IPC.
pub struct ServicoReal {
    canal: Mutex<Canal>,
    avisos: Arc<Mutex<VecDeque<Aviso>>>,
    conectado: Arc<AtomicBool>,
}

impl std::fmt::Debug for ServicoReal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ServicoReal")
            .field("conectado", &self.conectado.load(Ordering::Relaxed))
            .finish_non_exhaustive()
    }
}

impl ServicoReal {
    /// Conecta ao serviço e começa a ouvir.
    ///
    /// Manda [`Pedido::Acompanhar`] logo na conexão, para o serviço passar a empurrar avisos —
    /// sem isso, o código de pareamento nunca chegaria à tela.
    ///
    /// # Errors
    ///
    /// Erro de E/S se o serviço não estiver no ar ou o canal não puder ser aberto. Quem chama
    /// (o `main`) cai para o simulado nesse caso.
    pub fn conectar() -> std::io::Result<Self> {
        let (escrita, leitura) = conectar_canal(&endereco())?;
        let avisos = Arc::new(Mutex::new(VecDeque::new()));
        let conectado = Arc::new(AtomicBool::new(true));
        let (resp_tx, respostas) = std::sync::mpsc::channel();
        iniciar_leitor(
            leitura,
            resp_tx,
            Arc::clone(&avisos),
            Arc::clone(&conectado),
        );

        let servico = Self {
            canal: Mutex::new(Canal { escrita, respostas }),
            avisos,
            conectado,
        };
        // Pede para acompanhar: a partir daqui os avisos (código, conclusão) chegam sozinhos.
        let _ = servico.pedir(Pedido::Acompanhar);
        Ok(servico)
    }
}

impl Servico for ServicoReal {
    fn pedir(&self, pedido: Pedido) -> Resposta {
        let Ok(mut canal) = self.canal.lock() else {
            return Resposta::Falha(ir_ipc::Falha::Interna);
        };
        let Ok(quadro) = codec::codificar(&pedido) else {
            return Resposta::Falha(ir_ipc::Falha::Interna);
        };
        if canal.escrita.write_all(&quadro).is_err() || canal.escrita.flush().is_err() {
            self.conectado.store(false, Ordering::Relaxed);
            return Resposta::Falha(ir_ipc::Falha::Interna);
        }
        let Ok(resposta) = canal.respostas.recv_timeout(ESPERA) else {
            self.conectado.store(false, Ordering::Relaxed);
            return Resposta::Falha(ir_ipc::Falha::Interna);
        };
        resposta
    }

    fn avisos(&self) -> Vec<Aviso> {
        self.avisos
            .lock()
            .map(|mut fila| fila.drain(..).collect())
            .unwrap_or_default()
    }

    fn autoridade(&self) -> Autoridade {
        // O transporte local ainda não confere a elevação do processo, e o gate real do
        // pareamento é a comparação dos seis dígitos, que não é pulável. Declarar `Elevado` deixa
        // a interface oferecer o pareamento; a conferência de elevação no transporte é o
        // endurecimento da etapa de instalação como serviço.
        Autoridade::Elevado
    }

    fn simulado(&self) -> bool {
        false
    }
}

/// A thread que lê o canal e separa respostas de avisos.
fn iniciar_leitor(
    mut leitura: Box<dyn Read + Send>,
    respostas: Sender<Resposta>,
    avisos: Arc<Mutex<VecDeque<Aviso>>>,
    conectado: Arc<AtomicBool>,
) {
    std::thread::spawn(move || {
        loop {
            match ler_quadro(&mut leitura) {
                Ok(Some(ParaInterface::Resposta(resposta))) => {
                    if respostas.send(resposta).is_err() {
                        break;
                    }
                }
                Ok(Some(ParaInterface::Aviso(aviso))) => {
                    if let Ok(mut fila) = avisos.lock() {
                        fila.push_back(aviso);
                    }
                }
                // Variante futura do envelope, ainda não conhecida por esta versão: ignorar.
                Ok(Some(_)) => {}
                // Fim de fluxo limpo ou erro de leitura: o serviço fechou o canal.
                Ok(None) | Err(_) => break,
            }
        }
        conectado.store(false, Ordering::Relaxed);
    });
}

/// Lê um quadro com prefixo de tamanho, bloqueante. `Ok(None)` no fim limpo do fluxo.
fn ler_quadro(leitura: &mut impl Read) -> std::io::Result<Option<ParaInterface>> {
    let mut prefixo = [0u8; PREFIXO];
    if let Err(erro) = leitura.read_exact(&mut prefixo) {
        return if erro.kind() == std::io::ErrorKind::UnexpectedEof {
            Ok(None)
        } else {
            Err(erro)
        };
    }
    let tamanho = codec::tamanho_anunciado(&prefixo)
        .map_err(|_| std::io::Error::from(std::io::ErrorKind::InvalidData))?;
    let mut corpo = vec![0u8; tamanho];
    leitura.read_exact(&mut corpo)?;
    codec::decodificar(&corpo)
        .map(Some)
        .map_err(|_| std::io::Error::from(std::io::ErrorKind::InvalidData))
}

/// O endereço do canal de controle, igual ao do serviço, com o mesmo `IR_CONTROL_ENDPOINT`.
///
/// Público porque quem não conseguiu conectar precisa poder dizer **onde** procurou: "o serviço
/// não respondeu" sem o endereço manda a pessoa adivinhar.
#[must_use]
pub fn endereco_do_servico() -> String {
    endereco()
}

/// O endereço do canal de controle, igual ao do serviço, com o mesmo `IR_CONTROL_ENDPOINT`.
///
/// A sobrescrita aceita caminho completo ou nome curto, **exatamente como no serviço**: um valor
/// sem separador vira `\\.\pipe\<nome>` no Windows e um socket em `TMP` no Linux. Interpretar o
/// mesmo `IR_CONTROL_ENDPOINT` de dois jeitos diferentes faria a interface procurar o serviço num
/// lugar em que ele não está — e o sintoma seria a janela cair para o simulado sem explicação.
fn endereco() -> String {
    match std::env::var("IR_CONTROL_ENDPOINT") {
        Ok(valor) if !valor.is_empty() => expandir(&valor),
        _ => padrao(),
    }
}

/// Expande uma sobrescrita curta para um endereço completo da plataforma.
fn expandir(valor: &str) -> String {
    if valor.contains(['\\', '/']) {
        return valor.to_owned();
    }
    #[cfg(windows)]
    {
        format!(r"\\.\pipe\{valor}")
    }
    #[cfg(not(windows))]
    {
        std::env::temp_dir()
            .join(format!("{valor}.sock"))
            .to_string_lossy()
            .into_owned()
    }
}

/// O endereço padrão da plataforma.
fn padrao() -> String {
    #[cfg(windows)]
    {
        r"\\.\pipe\inputremote-control".to_owned()
    }
    #[cfg(not(windows))]
    {
        "/run/inputremote/control.sock".to_owned()
    }
}

/// Abre a conexão e devolve as duas metades (escrita e leitura) sobre o mesmo canal duplex.
#[cfg(windows)]
fn conectar_canal(
    endereco: &str,
) -> std::io::Result<(Box<dyn Write + Send>, Box<dyn Read + Send>)> {
    use std::fs::OpenOptions;
    let escrita = OpenOptions::new().read(true).write(true).open(endereco)?;
    let leitura = escrita.try_clone()?;
    Ok((Box::new(escrita), Box::new(leitura)))
}

/// Abre a conexão e devolve as duas metades (escrita e leitura) sobre o mesmo canal duplex.
#[cfg(not(windows))]
fn conectar_canal(
    endereco: &str,
) -> std::io::Result<(Box<dyn Write + Send>, Box<dyn Read + Send>)> {
    use std::os::unix::net::UnixStream;
    let escrita = UnixStream::connect(endereco)?;
    let leitura = escrita.try_clone()?;
    Ok((Box::new(escrita), Box::new(leitura)))
}
