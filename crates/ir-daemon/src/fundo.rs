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

/// Leva ao ator, como resultado de fundo, tudo o que chegar por `origem` — até o ator ir embora.
///
/// Um laço só para as três origens (sistema, rádio tardio, tela). O do rádio tardio levava só o
/// primeiro: o rádio perdido e reaberto uma segunda vez chegava ao canal e ninguém o lia.
fn repassar<T: Send + 'static>(
    mut origem: mpsc::UnboundedReceiver<T>,
    de_fundo: mpsc::UnboundedSender<actor::DeFundo>,
    embrulho: fn(T) -> actor::DeFundo,
) {
    tokio::spawn(async move {
        while let Some(valor) = origem.recv().await {
            if de_fundo.send(embrulho(valor)).is_err() {
                return;
            }
        }
    });
}

/// Leva ao ator o que o sistema avisa — pelo SCM no Windows, por sinais no Linux.
pub(crate) fn repassar_sistema(
    sistema: mpsc::UnboundedReceiver<ir_servico::EventoDoSistema>,
    de_fundo: &mpsc::UnboundedSender<actor::DeFundo>,
) {
    #[cfg(unix)]
    {
        let (avisos, ouvidos) = mpsc::unbounded_channel();
        ir_servico::sinais::ouvir_o_gancho_de_suspensao(avisos);
        repassar(ouvidos, de_fundo.clone(), actor::DeFundo::Sistema);
    }
    repassar(sistema, de_fundo.clone(), actor::DeFundo::Sistema);
}

/// Repassa ao ator o rádio que abrir depois da subida — ou que voltar depois de perdido —, como
/// mais um resultado de fundo.
pub(crate) fn repassar_radio_tardio(
    tardio: mpsc::UnboundedReceiver<ir_transporte::RadioAberto>,
    de_fundo: &mpsc::UnboundedSender<actor::DeFundo>,
) {
    repassar(tardio, de_fundo.clone(), actor::DeFundo::Radio);
}

/// Vigia se a tela desta máquina está bloqueada, ou no login, pelo `logind` (Linux), e leva ao ator.
#[cfg(target_os = "linux")]
pub(crate) fn vigiar_a_tela(de_fundo: &mpsc::UnboundedSender<actor::DeFundo>) {
    let (tela, mudou) = mpsc::unbounded_channel();
    ir_servico::logind::vigiar_a_tela(tela);
    repassar(mudou, de_fundo.clone(), actor::DeFundo::TelaProtegida);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn repassa_tudo_o_que_chega_e_nao_so_o_primeiro() {
        // O rádio perdido e reaberto pela segunda vez chegava ao canal, e ninguém mais o lia.
        let (origem, recebe) = mpsc::unbounded_channel();
        let (de_fundo, mut chegam) = mpsc::unbounded_channel();
        repassar(recebe, de_fundo, actor::DeFundo::TelaProtegida);
        for protegida in [true, false, true] {
            let _ = origem.send(protegida);
        }
        for esperada in [true, false, true] {
            let chegou = chegam.recv().await;
            assert!(
                matches!(chegou, Some(actor::DeFundo::TelaProtegida(p)) if p == esperada),
                "{chegou:?}"
            );
        }
    }
}
