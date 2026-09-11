//! O agente de sessão: captura e injeta **dentro da sessão do usuário**, a mando do serviço.
//!
//! Ele existe por uma limitação dura do Windows: um serviço na **sessão 0** não enxerga o
//! teclado e o mouse do usuário, e o `SendInput` dele não chega ao desktop de ninguém
//! ([05, §3](../../../docs/05-windows.md)). Quem captura e injeta precisa nascer do lado certo,
//! e é o serviço que o lança lá.
//!
//! # O agente não decide nada
//!
//! Ele recebe "injete isto" e devolve "isto aconteceu"
//! ([02, §1.2](../../../docs/02-arquitetura.md)). Toda política vive no serviço — e é isso que
//! permite o agente morrer e ressubir sem consequência: o serviço o relança, e o estado, que
//! nunca esteve aqui, continua íntegro.
//!
//! # Por que sem `tokio`
//!
//! Os ganchos de baixo nível já rodam numa thread própria com laço de mensagens, e o trabalho
//! aqui é bloqueante por natureza: uma thread escreve os fatos, a principal lê os comandos. Um
//! runtime assíncrono não acrescentaria nada e só daria mais uma coisa para dar errado.

use std::io::{Read, Write};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{Context, Result};
use ir_input::{CaptureEvent, Capturer, InjectEvent, Injector};
use ir_ipc::codec::{self, PREFIXO};
use ir_ipc::{ComandoDoAgente, FatoDoAgente};
use ir_proto::input::PointerPosition;
use ir_proto::screens::ScreenLayout;
use tracing::{info, warn};

/// Quanto tempo se insiste em achar o serviço antes de desistir.
///
/// O serviço pode estar subindo junto (no arranque da máquina, os dois nascem quase juntos).
/// Desistir cedo faria o agente morrer no boot e só voltar na próxima tentativa do serviço.
const TENTATIVAS: u32 = 60;
/// Intervalo entre tentativas de conexão.
const ESPERA: Duration = Duration::from_millis(500);

fn main() {
    iniciar_tracing();
    info!("agente do InputRemote iniciando");

    // Uma sessão só. Se a conexão cai, o agente **sai**: quem o ressobe é o serviço, e sair é
    // mais seguro que tentar remendar um estado que não é nosso.
    match servir() {
        Ok(()) => info!("o serviço encerrou a conexão; saindo"),
        Err(erro) => warn!(%erro, "o agente terminou com erro"),
    }
}

/// Configura o `tracing`, com nível de `RUST_LOG` ou `info` por padrão.
fn iniciar_tracing() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();
}

/// Conecta ao serviço, liga entrada e serve comandos até a conexão cair.
fn servir() -> Result<()> {
    let (escrita, mut leitura) = conectar()?;
    let escrita = Arc::new(Mutex::new(escrita));
    info!("conectado ao serviço");

    // A injeção é o papel do lado controlado; a captura, o do lado que tem o teclado. O agente
    // liga os dois e deixa o serviço decidir qual usar — ele não sabe qual papel esta máquina
    // tem, e não é dele decidir.
    let injetor = abrir_injetor();
    let (capturador, eventos) = abrir_captura();

    anunciar(&escrita, capturador.is_some())?;
    if let Some(eventos) = eventos {
        bombear_captura(eventos, Arc::clone(&escrita));
    }

    let mut estado = Entrada {
        injetor,
        capturador,
    };
    // O laço principal: um comando de cada vez, na ordem em que o serviço mandou.
    while let Some(comando) = ler_quadro::<ComandoDoAgente>(&mut leitura)? {
        if matches!(comando, ComandoDoAgente::Encerrar) {
            break;
        }
        estado.executar(comando, &escrita);
    }

    // Saída limpa: nada pode ficar pressionado quando o agente vai embora.
    estado.soltar_tudo();
    Ok(())
}

/// O que o agente tem para mexer na máquina local.
struct Entrada {
    injetor: Option<Box<dyn Injector>>,
    capturador: Option<Box<dyn Capturer>>,
}

impl Entrada {
    /// Executa um comando do serviço.
    fn executar(&mut self, comando: ComandoDoAgente, escrita: &Arc<Mutex<Escrita>>) {
        match comando {
            ComandoDoAgente::SoltarTudo => self.soltar_tudo(),
            ComandoDoAgente::SuprimirEntradaLocal(ligado) => {
                if let Some(capturador) = self.capturador.as_ref() {
                    capturador.set_suppress(ligado);
                }
            }
            ComandoDoAgente::PrenderPonteiro(posicao) => self.prender(posicao),
            // A Sequência de Atenção Segura é N2: depende de `SendSAS` e da política do sistema
            // ([05, §4.3](../../../docs/05-windows.md)), e entra com a tela de bloqueio.
            ComandoDoAgente::SequenciaDeAtencao => {
                warn!("Ctrl+Alt+Del pedido, mas ainda não implementado");
            }
            outro => self.injetar(outro, escrita),
        }
    }

    /// Injeta o que o comando pedir, e conta ao serviço se o sistema recusar.
    fn injetar(&mut self, comando: ComandoDoAgente, escrita: &Arc<Mutex<Escrita>>) {
        let Some(evento) = para_evento(comando) else {
            return;
        };
        let Some(injetor) = self.injetor.as_mut() else {
            return;
        };
        if let Err(erro) = injetor.inject(evento) {
            // Do ponto de vista do usuário, nada aconteceu — ele não teria como saber. Por isso
            // a recusa é contada, e não só registrada ([05, §4.4](../../../docs/05-windows.md)).
            warn!(%erro, "injeção recusada pelo sistema");
            let _ = enviar(
                escrita,
                &FatoDoAgente::InjecaoRecusada {
                    desktop: "Default".to_owned(),
                },
            );
        }
    }

    /// Põe o ponteiro local na posição normalizada, convertida para pixels desta tela.
    ///
    /// A conversão é feita **aqui**, e não no serviço: quem sabe o tamanho da tela do usuário é
    /// quem está na sessão dele. Um serviço na sessão 0 leria métricas que não são as dele.
    fn prender(&self, posicao: PointerPosition) {
        let Some(capturador) = self.capturador.as_ref() else {
            return;
        };
        let (largura, altura) = ir_input::primary_screen_size().unwrap_or((1920, 1080));
        let x = i32::try_from(u32::from(posicao.x) * largura / 65_535).unwrap_or(0);
        let y = i32::try_from(u32::from(posicao.y) * altura / 65_535).unwrap_or(0);
        capturador.warp_pointer(x, y);
    }

    /// Solta tudo que possa estar pressionado. O comando mais importante do produto.
    fn soltar_tudo(&mut self) {
        if let Some(injetor) = self.injetor.as_mut() {
            let _ = injetor.release_all();
        }
        if let Some(capturador) = self.capturador.as_ref() {
            // Se caímos com a supressão ligada, o teclado do usuário ficaria morto.
            capturador.set_suppress(false);
        }
    }
}

/// Converte um comando de injeção no evento do backend de entrada.
fn para_evento(comando: ComandoDoAgente) -> Option<InjectEvent> {
    Some(match comando {
        ComandoDoAgente::Tecla { usage, pressionada } => InjectEvent::Key {
            usage,
            pressed: pressionada,
        },
        ComandoDoAgente::Botao { botao, pressionado } => InjectEvent::Button {
            button: botao,
            pressed: pressionado,
        },
        ComandoDoAgente::Roda(delta) => InjectEvent::Wheel(delta),
        ComandoDoAgente::Ponteiro(posicao) => InjectEvent::Pointer(posicao),
        _ => return None,
    })
}

/// Abre o injetor, sem derrubar o agente se não der.
fn abrir_injetor() -> Option<Box<dyn Injector>> {
    match ir_input::open_injector() {
        Ok(injetor) => Some(injetor),
        Err(erro) => {
            warn!(%erro, "sem injeção nesta máquina");
            None
        }
    }
}

/// Liga a captura, sem derrubar o agente se não der.
fn abrir_captura() -> (
    Option<Box<dyn Capturer>>,
    Option<std::sync::mpsc::Receiver<CaptureEvent>>,
) {
    let (tx, rx) = std::sync::mpsc::channel();
    match ir_input::start_capture(tx) {
        Ok(capturador) => (Some(capturador), Some(rx)),
        Err(erro) => {
            warn!(%erro, "sem captura nesta máquina");
            (None, None)
        }
    }
}

/// Conta ao serviço que estamos prontos, e qual é o arranjo de telas desta sessão.
fn anunciar(escrita: &Arc<Mutex<Escrita>>, capturando: bool) -> Result<()> {
    let desktops = if capturando {
        vec!["Default".to_owned()]
    } else {
        Vec::new()
    };
    enviar(escrita, &FatoDoAgente::Pronto { desktops })?;

    // O tamanho da tela vem de quem está na sessão do usuário. É o dado que faz a travessia
    // cair na borda certa, e o serviço na sessão 0 não tem como saber sozinho.
    if let Some((largura, altura)) = ir_input::primary_screen_size()
        && let Ok(arranjo) = ScreenLayout::single(largura, altura)
    {
        info!(largura, altura, "tela da sessão do usuário");
        enviar(escrita, &FatoDoAgente::TelasMudaram(arranjo))?;
    }
    Ok(())
}

/// A thread que leva os eventos capturados para o serviço.
fn bombear_captura(eventos: std::sync::mpsc::Receiver<CaptureEvent>, escrita: Arc<Mutex<Escrita>>) {
    std::thread::spawn(move || {
        while let Ok(evento) = eventos.recv() {
            let Some(fato) = para_fato(evento) else {
                continue;
            };
            if enviar(&escrita, &fato).is_err() {
                break; // o serviço foi embora; a thread principal também vai perceber
            }
        }
    });
}

/// Converte um evento capturado no fato que o serviço entende.
fn para_fato(evento: CaptureEvent) -> Option<FatoDoAgente> {
    Some(match evento {
        CaptureEvent::PointerMotion { dx, dy } => FatoDoAgente::PonteiroLocal { dx, dy },
        CaptureEvent::PointerAbsolute { x, y } => FatoDoAgente::PonteiroAbsoluto { x, y },
        CaptureEvent::Wheel(delta) => FatoDoAgente::RodaLocal(delta),
        CaptureEvent::Key { usage, pressed } => FatoDoAgente::TeclaLocal {
            usage,
            pressionada: pressed,
        },
        CaptureEvent::Button { button, pressed } => FatoDoAgente::BotaoLocal {
            botao: button,
            pressionado: pressed,
        },
        _ => return None,
    })
}

/// A metade de escrita do canal, sob cadeado (duas threads escrevem nela).
type Escrita = Box<dyn Write + Send>;

/// Manda um fato ao serviço.
fn enviar(escrita: &Arc<Mutex<Escrita>>, fato: &FatoDoAgente) -> Result<()> {
    let quadro = codec::codificar(fato).context("codificando o fato")?;
    let mut guarda = escrita
        .lock()
        .map_err(|_| anyhow::anyhow!("o canal de escrita foi envenenado"))?;
    guarda.write_all(&quadro).context("escrevendo no canal")?;
    guarda.flush().context("esvaziando o canal")?;
    Ok(())
}

/// Lê um quadro do canal. `Ok(None)` no fim limpo.
fn ler_quadro<T: serde::de::DeserializeOwned>(leitura: &mut impl Read) -> Result<Option<T>> {
    let mut prefixo = [0u8; PREFIXO];
    if let Err(erro) = leitura.read_exact(&mut prefixo) {
        return if erro.kind() == std::io::ErrorKind::UnexpectedEof {
            Ok(None)
        } else {
            Err(erro.into())
        };
    }
    // O tamanho é conferido contra o limite antes de alocar.
    let tamanho = codec::tamanho_anunciado(&prefixo).context("prefixo inválido")?;
    let mut corpo = vec![0u8; tamanho];
    leitura.read_exact(&mut corpo).context("corpo incompleto")?;
    Ok(Some(codec::decodificar(&corpo).context("quadro inválido")?))
}

/// O endereço do canal do agente, igual ao do serviço.
fn endereco() -> String {
    if let Ok(valor) = std::env::var("IR_AGENT_ENDPOINT")
        && !valor.is_empty()
    {
        let curto = !valor.contains(['\\', '/']);
        if !curto {
            return valor;
        }
        #[cfg(windows)]
        {
            return format!(r"\\.\pipe\{valor}");
        }
        #[cfg(not(windows))]
        {
            return std::env::temp_dir()
                .join(format!("{valor}.sock"))
                .to_string_lossy()
                .into_owned();
        }
    }
    #[cfg(windows)]
    {
        r"\\.\pipe\inputremote-agent".to_owned()
    }
    #[cfg(not(windows))]
    {
        "/run/inputremote/agent.sock".to_owned()
    }
}

/// Conecta ao canal do agente, insistindo enquanto o serviço não abre.
fn conectar() -> Result<(Escrita, Box<dyn Read + Send>)> {
    let endereco = endereco();
    let mut ultimo = None;
    for _ in 0..TENTATIVAS {
        match abrir(&endereco) {
            Ok(par) => return Ok(par),
            Err(erro) => ultimo = Some(erro),
        }
        std::thread::sleep(ESPERA);
    }
    let erro = ultimo.unwrap_or_else(|| std::io::Error::other("sem tentativa"));
    Err(anyhow::Error::new(erro).context(format!("não achei o serviço em {endereco}")))
}

/// Abre a conexão e devolve as duas metades sobre o mesmo canal duplex.
#[cfg(windows)]
fn abrir(endereco: &str) -> std::io::Result<(Escrita, Box<dyn Read + Send>)> {
    use std::fs::OpenOptions;
    let escrita = OpenOptions::new().read(true).write(true).open(endereco)?;
    let leitura = escrita.try_clone()?;
    Ok((Box::new(escrita), Box::new(leitura)))
}

/// Abre a conexão e devolve as duas metades sobre o mesmo canal duplex.
#[cfg(not(windows))]
fn abrir(endereco: &str) -> std::io::Result<(Escrita, Box<dyn Read + Send>)> {
    use std::os::unix::net::UnixStream;
    let escrita = UnixStream::connect(endereco)?;
    let leitura = escrita.try_clone()?;
    Ok((Box::new(escrita), Box::new(leitura)))
}
