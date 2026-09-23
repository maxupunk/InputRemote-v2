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
//! runtime assíncrono não acrescentaria nada ao agente e só daria mais uma coisa para dar errado.
//!
//! Uma thread lendo e outra escrevendo no mesmo canal é, porém, exatamente o que um *named pipe*
//! síncrono do Windows não aguenta: a escrita espera a leitura pendente terminar, e o movimento do
//! mouse capturado só saía quando o serviço mandava algum comando. Por isso o canal é aberto por
//! [`ir_ipc::cliente`], que usa E/S sobreposta por baixo e entrega aqui um `Read` e um `Write`
//! bloqueantes como antes (log 21).

// Sem janela de console em release. O serviço lança o agente e o ajudante de clipboard com
// `CREATE_NO_WINDOW`, mas quem abre o ajudante à mão (ou uma versão antiga, pela chave `Run`) veria
// uma janela preta na tela, o mesmo defeito que a interface teve (log 12).
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::io::{Read, Write};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{Context, Result};
use ir_input::{CaptureEvent, Capturer, Injector};
use ir_ipc::codec::{self, PREFIXO};
use ir_ipc::{ComandoDoAgente, FatoDoAgente};
use ir_proto::screens::ScreenLayout;
use tracing::{info, warn};

mod clipboard;
mod entrada;
mod registro;
mod vigia;

/// Quanto tempo se insiste em achar o serviço antes de desistir.
///
/// O serviço pode estar subindo junto (no arranque da máquina, os dois nascem quase juntos).
/// Desistir cedo faria o agente morrer no boot e só voltar na próxima tentativa do serviço.
const TENTATIVAS: u32 = 60;
/// Intervalo entre tentativas de conexão.
const ESPERA: Duration = Duration::from_millis(500);

fn main() {
    // O guarda esvazia a fila do registro ao sair; soltá-lo antes perderia as últimas linhas.
    let _registro = registro::iniciar();

    // O ajudante de clipboard é o mesmo executável num papel diferente: roda **como o usuário**,
    // iniciado pela sessão, e fala pelo canal de controle (ADR-0011). Um binário a menos para
    // instalar e assinar.
    if std::env::args().any(|argumento| argumento == "--clipboard") {
        info!("ajudante de clipboard do InputRemote iniciando");
        if let Err(erro) = clipboard::servir() {
            warn!(%erro, "o ajudante de clipboard terminou");
        }
        return;
    }

    info!("agente do InputRemote iniciando");

    // Uma sessão só. Se a conexão cai, o agente **sai**: quem o ressobe é o serviço, e sair é
    // mais seguro que tentar remendar um estado que não é nosso.
    match servir() {
        Ok(()) => info!("o serviço encerrou a conexão; saindo"),
        Err(erro) => warn!(%erro, "o agente terminou com erro"),
    }
}

/// Conecta ao serviço, liga entrada e serve comandos até a conexão cair.
fn servir() -> Result<()> {
    let (escrita, leitura) = conectar()?;
    let escrita = Arc::new(Mutex::new(escrita));
    info!("conectado ao serviço");

    // A injeção é o papel do lado controlado; a captura, o do lado que tem o teclado. O agente
    // liga os dois e deixa o serviço decidir qual usar — ele não sabe qual papel esta máquina
    // tem, e não é dele decidir.
    let injetor = abrir_injetor();
    let (capturador, eventos) = abrir_captura();

    let desktops = injetor
        .as_ref()
        .map(|injetor| injetor.desktops())
        .unwrap_or_default();
    anunciar(&escrita, capturador.is_some(), desktops)?;
    if let Some(eventos) = eventos {
        bombear_captura(eventos, Arc::clone(&escrita));
    }

    // Nada pode ficar pressionado quando o agente vai embora, por qualquer saída: o `Drop` de
    // `Entrada` solta tudo, inclusive quando a leitura falha no meio.
    let mut estado = entrada::Entrada {
        injetor,
        capturador,
    };
    vigiar_o_desktop(Arc::clone(&escrita));
    let comandos = ler_comandos(leitura);
    let mut vigia = vigia::Vigia::default();
    // O laço principal: um comando de cada vez, na ordem em que o serviço mandou — com prazo, para
    // o vigia poder agir quando o serviço se cala.
    loop {
        match comandos.recv_timeout(Duration::from_millis(250)) {
            Ok(Ok(ComandoDoAgente::Encerrar))
            | Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => return Ok(()),
            Ok(Ok(comando)) => {
                vigia.viu(comando, std::time::Instant::now());
                estado.executar(comando, &escrita);
            }
            Ok(Err(erro)) => return Err(erro),
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
        }
        if vigia.venceu(std::time::Instant::now()) {
            warn!("o serviço parou de renovar a supressão; devolvendo o teclado e o mouse daqui");
            estado.soltar_tudo();
        }
    }
}

/// A thread que conta ao serviço quando o desktop de entrada muda, e quando os monitores mudam.
///
/// A tela que bloqueia não passa pelos ganchos — nem o Win+L, nem a troca para o desktop seguro
/// ([05, §5.1](../../../docs/05-windows.md)) —, então quem percebe é esta pergunta periódica. Do
/// lado que controla, é o que devolve o controle quando a tela daqui bloqueia; do controlado, o que
/// encerra a recusa quando a pessoa volta à área de trabalho. Só no Windows: lá é que há desktops.
fn vigiar_o_desktop(escrita: Arc<Mutex<Escrita>>) {
    if ir_input::desktop_de_entrada().is_none() {
        return;
    }
    std::thread::spawn(move || {
        let mut anterior = ir_input::desktop_de_entrada();
        let mut telas = arranjo_da_sessao();
        for volta in 1u32.. {
            std::thread::sleep(Duration::from_millis(200));
            // Um monitor ligado, desligado ou rearranjado: a cada dois segundos, o arranjo de novo.
            if volta.is_multiple_of(10) && !contar_telas_se_mudaram(&escrita, &mut telas) {
                return; // o serviço foi embora
            }
            let agora = ir_input::desktop_de_entrada();
            if agora.is_none() || agora == anterior {
                continue;
            }
            if let Some(nome) = agora.clone()
                && enviar(&escrita, &FatoDoAgente::DesktopMudou { nome }).is_err()
            {
                return; // o serviço foi embora
            }
            anterior = agora;
        }
    });
}

/// Conta o arranjo de telas ao serviço se ele mudou. `false` se o serviço foi embora.
fn contar_telas_se_mudaram(
    escrita: &Arc<Mutex<Escrita>>,
    telas: &mut Option<ScreenLayout>,
) -> bool {
    let agora = arranjo_da_sessao();
    if agora.is_none() || agora == *telas {
        return true;
    }
    telas.clone_from(&agora);
    agora.is_none_or(|arranjo| enviar(escrita, &FatoDoAgente::TelasMudaram(arranjo)).is_ok())
}

/// A thread que lê os comandos do serviço. O canal fecha no fim limpo; um erro vai por ele.
fn ler_comandos(
    mut leitura: impl Read + Send + 'static,
) -> std::sync::mpsc::Receiver<Result<ComandoDoAgente>> {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        loop {
            match ler_quadro::<ComandoDoAgente>(&mut leitura) {
                Ok(Some(comando)) => {
                    if tx.send(Ok(comando)).is_err() {
                        return;
                    }
                }
                Ok(None) => return,
                Err(erro) => {
                    let _ = tx.send(Err(erro));
                    return;
                }
            }
        }
    });
    rx
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
fn anunciar(
    escrita: &Arc<Mutex<Escrita>>,
    capturando: bool,
    mut desktops: Vec<String>,
) -> Result<()> {
    // Os desktops em que o agente injeta; sem a lista do injetor, a área de trabalho se ele captura.
    if desktops.is_empty() && capturando {
        desktops.push("Default".to_owned());
    }
    enviar(escrita, &FatoDoAgente::Pronto { desktops })?;

    // O arranjo de telas vem de quem está na sessão do usuário. É o dado que faz a travessia cair
    // na borda certa, e o serviço na sessão 0 não tem como saber sozinho. Todos os monitores, e não
    // só o principal: com dois, o segundo não existia para o par.
    if let Some(arranjo) = arranjo_da_sessao() {
        info!(monitores = arranjo.len(), "telas da sessão do usuário");
        enviar(escrita, &FatoDoAgente::TelasMudaram(arranjo))?;
    }
    Ok(())
}

/// O arranjo de telas desta sessão: todos os monitores, ou pelo menos a tela principal.
fn arranjo_da_sessao() -> Option<ScreenLayout> {
    ir_input::arranjo_de_telas().or_else(|| {
        let (largura, altura) = ir_input::primary_screen_size()?;
        ScreenLayout::single(largura, altura).ok()
    })
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
pub(crate) type Escrita = Box<dyn Write + Send>;

/// Manda um fato ao serviço.
pub(crate) fn enviar(escrita: &Arc<Mutex<Escrita>>, fato: &FatoDoAgente) -> Result<()> {
    let quadro = codec::codificar(fato).context("codificando o fato")?;
    let mut guarda = escrita
        .lock()
        .map_err(|_| anyhow::anyhow!("o canal de escrita foi envenenado"))?;
    guarda.write_all(&quadro).context("escrevendo no canal")?;
    guarda.flush().context("esvaziando o canal")?;
    Ok(())
}

/// Lê um quadro do canal. `Ok(None)` no fim limpo.
///
/// Serve aos dois canais: o do agente e o de controle, que o ajudante de clipboard usa.
pub(crate) fn ler_quadro<T: serde::de::DeserializeOwned>(
    leitura: &mut impl Read,
) -> Result<Option<T>> {
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
///
/// As duas metades são usadas ao mesmo tempo — a principal presa lendo comandos, a de captura
/// escrevendo fatos —, e é por isso que o canal vem do cliente compartilhado de `ir-ipc`.
fn abrir(endereco: &str) -> std::io::Result<(Escrita, Box<dyn Read + Send>)> {
    ir_ipc::cliente::abrir(endereco)
}
