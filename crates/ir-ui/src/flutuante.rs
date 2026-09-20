//! O aviso de cópia no canto da tela.
//!
//! Quem copia está no Explorer ou no Nautilus, com a janela do InputRemote fechada. Um retorno
//! que só existisse dentro dela não seria retorno: a pessoa copiava, ia colar do outro lado, e
//! descobria pela pasta errada que nada tinha atravessado. Este aviso aparece sozinho enquanto a
//! cópia anda, mostra o fim dela por alguns segundos, e some.
//!
//! Ele nunca rouba o foco nem pede clique: é recado, não janela de trabalho.
//!
//! O único `unsafe` da interface está aqui, numa linha: perguntar ao Windows o tamanho da tela
//! (`GetSystemMetrics`), que é leitura pura e sem pré-condição. Sem ele não há canto inferior
//! direito — o Slint sabe o tamanho da própria janela, e não o da tela.

#![allow(unsafe_code)]

use std::rc::Rc;
use std::time::Duration;

use slint::{ComponentHandle, LogicalPosition, Timer, TimerMode};

use crate::gerado::{CopiaUi, Flutuante};

/// Quanto tempo o resultado fica na tela depois que a cópia termina.
///
/// Tempo de ler duas linhas sem pressa. Enquanto a cópia anda, o aviso fica — o prazo só começa
/// no fim.
const PERMANENCIA: Duration = Duration::from_secs(6);

/// A distância das bordas da tela, para o aviso não colar no canto.
const MARGEM: f32 = 24.0;

/// O aviso, criado sob demanda e reaproveitado nas cópias seguintes.
pub(crate) struct Aviso {
    janela: Option<Flutuante>,
    /// O relógio que o esconde depois do fim. Guardado porque um `Timer` solto é cancelado.
    prazo: Rc<Timer>,
}

impl Aviso {
    /// Um aviso ainda sem janela: ela nasce na primeira cópia.
    pub(crate) fn novo() -> Self {
        Self {
            janela: None,
            prazo: Rc::new(Timer::default()),
        }
    }

    /// Mostra esta cópia. `terminou` liga o prazo para o aviso sumir.
    pub(crate) fn mostrar(&mut self, copia: CopiaUi, terminou: bool) {
        let janela = match self.janela.as_ref() {
            Some(janela) => janela,
            None => match Flutuante::new() {
                Ok(nova) => self.janela.insert(nova),
                Err(erro) => {
                    // Sem aviso flutuante o produto continua copiando, e a janela mostra o mesmo
                    // texto. Não é motivo para derrubar a interface.
                    eprintln!("não consegui abrir o aviso de cópia: {erro}");
                    return;
                }
            },
        };
        janela.set_copia(copia);
        // Quem está em foco continua em foco: uma janela nova o toma, e tomar o foco de quem
        // está digitando por causa de um recado seria pior que não dar o recado.
        let anterior = em_foco();
        if janela.show().is_err() {
            return;
        }
        posicionar(janela);
        devolver_foco(anterior);

        self.prazo.stop();
        if !terminou {
            return;
        }
        let fraca = janela.as_weak();
        self.prazo
            .start(TimerMode::SingleShot, PERMANENCIA, move || {
                if let Some(janela) = fraca.upgrade() {
                    let _ = janela.hide();
                }
            });
    }
}

/// Põe o aviso no canto inferior direito da tela.
///
/// Sem tamanho de tela, deixa onde está: um aviso no lugar errado ainda é melhor que nenhum.
fn posicionar(janela: &Flutuante) {
    let escala = janela.window().scale_factor();
    let Some((largura_da_tela, altura_da_tela)) = tela() else {
        return;
    };
    if escala <= 0.0 {
        return;
    }
    let tamanho = janela.window().size().to_logical(escala);
    let x = largura_da_tela / escala - tamanho.width - MARGEM;
    let y = altura_da_tela / escala - tamanho.height - MARGEM;
    janela
        .window()
        .set_position(LogicalPosition::new(x.max(0.0), y.max(0.0)));
}

/// Quem está em foco agora, para o foco voltar depois de o aviso aparecer.
fn em_foco() -> windows_sys::Win32::Foundation::HWND {
    // SAFETY: a chamada só lê qual janela está em primeiro plano e não tem pré-condição.
    unsafe { windows_sys::Win32::UI::WindowsAndMessaging::GetForegroundWindow() }
}

/// Devolve o foco a quem o tinha. Recusa do sistema não é problema: o aviso some sozinho.
fn devolver_foco(anterior: windows_sys::Win32::Foundation::HWND) {
    if anterior.is_null() {
        return;
    }
    // SAFETY: `anterior` é a janela que estava em primeiro plano um instante atrás; se ela já não
    // existir, a chamada apenas falha.
    unsafe {
        windows_sys::Win32::UI::WindowsAndMessaging::SetForegroundWindow(anterior);
    }
}

/// O tamanho da tela primária, em pixels físicos.
fn tela() -> Option<(f32, f32)> {
    use windows_sys::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_CXSCREEN, SM_CYSCREEN};
    // SAFETY: a chamada só lê uma métrica do sistema e não tem pré-condição.
    let (largura, altura) =
        unsafe { (GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN)) };
    if largura <= 0 || altura <= 0 {
        return None;
    }
    #[allow(clippy::cast_precision_loss)]
    Some((largura as f32, altura as f32))
}
