//! O que alimenta o ator por fora do laço: o terminal, o sistema operacional e o rádio que
//! abre tarde. Cada um vira um evento no canal do ator, na vez dele.

use std::io::BufRead;

use tokio::sync::mpsc;

use crate::actor;

/// A ponte da confirmação de pareamento: lê linhas do stdin numa thread própria.
pub(crate) fn spawn_stdin_reader() -> mpsc::UnboundedReceiver<String> {
    let (tx, rx) = mpsc::unbounded_channel();
    std::thread::spawn(move || {
        let stdin = std::io::stdin();
        for line in stdin.lock().lines().map_while(Result::ok) {
            if tx.send(line).is_err() {
                return;
            }
        }
        // Fim do stdin (redirecionado de um arquivo, por exemplo): segura o emissor para o canal
        // não fechar, senão o laço do ator giraria recebendo `None` sem parar.
        loop {
            std::thread::park();
        }
    });
    rx
}

/// Leva ao ator o que o sistema avisa — pelo SCM no Windows, por sinais no Linux.
pub(crate) fn repassar_sistema(
    mut sistema: mpsc::UnboundedReceiver<ir_servico::EventoDoSistema>,
    de_fundo: mpsc::UnboundedSender<actor::DeFundo>,
) {
    #[cfg(unix)]
    {
        let (avisos, mut ouvidos) = mpsc::unbounded_channel();
        ir_servico::sinais::ouvir_o_gancho_de_suspensao(avisos);
        let de_fundo = de_fundo.clone();
        tokio::spawn(async move {
            while let Some(evento) = ouvidos.recv().await {
                if de_fundo.send(actor::DeFundo::Sistema(evento)).is_err() {
                    return;
                }
            }
        });
    }
    tokio::spawn(async move {
        while let Some(evento) = sistema.recv().await {
            if de_fundo.send(actor::DeFundo::Sistema(evento)).is_err() {
                return;
            }
        }
    });
}

/// Repassa ao ator o rádio que abrir depois da subida, como mais um resultado de fundo.
pub(crate) fn repassar_radio_tardio(
    mut tardio: mpsc::UnboundedReceiver<ir_transporte::RadioAberto>,
    de_fundo: mpsc::UnboundedSender<actor::DeFundo>,
) {
    tokio::spawn(async move {
        if let Some(aberto) = tardio.recv().await {
            let _ = de_fundo.send(actor::DeFundo::Radio(aberto));
        }
    });
}

/// Vigia se a tela desta máquina está bloqueada, ou no login, pelo `logind` (Linux), e leva ao ator.
#[cfg(target_os = "linux")]
pub(crate) fn vigiar_a_tela(de_fundo: mpsc::UnboundedSender<actor::DeFundo>) {
    let (tela, mut mudou) = mpsc::unbounded_channel();
    ir_servico::logind::vigiar_a_tela(tela);
    tokio::spawn(async move {
        while let Some(protegida) = mudou.recv().await {
            if de_fundo
                .send(actor::DeFundo::TelaProtegida(protegida))
                .is_err()
            {
                return;
            }
        }
    });
}

/// Liga a captura local e a ponte da thread dela (`std`) para o canal do ator (`tokio`).
///
/// # Errors
///
/// A falha do backend: sem dispositivo legível, ou sem backend nesta plataforma.
#[cfg(not(windows))]
pub(crate) fn capturar(
    para_o_ator: &mpsc::UnboundedSender<ir_input::CaptureEvent>,
) -> ir_input::Result<Box<dyn ir_input::Capturer>> {
    let (std_tx, std_rx) = std::sync::mpsc::channel();
    let capturer = ir_input::start_capture(std_tx)?;
    let para_o_ator = para_o_ator.clone();
    std::thread::spawn(move || {
        while let Ok(evento) = std_rx.recv() {
            if para_o_ator.send(evento).is_err() {
                break;
            }
        }
    });
    Ok(capturer)
}
