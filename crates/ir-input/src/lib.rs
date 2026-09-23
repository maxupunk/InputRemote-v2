//! Captura e injeção de entrada, com um backend por sistema operacional.
//!
//! O crate expõe dois papéis, correspondendo aos dois lados de uma sessão:
//!
//! - [`Injector`] — o **cliente** injeta o que recebe. No Linux é `uinput`, que entra abaixo do
//!   compositor e funciona no greeter, na tela de bloqueio e no console
//!   ([06, §2](../../../docs/06-linux.md)). No Windows é `SendInput`.
//! - [`Capturer`] — o **servidor** captura teclado e mouse locais e suprime a entrada local
//!   enquanto o controle está no par. No Windows são os ganchos de baixo nível
//!   ([05, §5](../../../docs/05-windows.md)).
//!
//! # Fronteira
//!
//! Este é um crate de plataforma: ele tem `unsafe` (confinado aos módulos de backend, cada um
//! com `#[allow(unsafe_code)]` e comentário `// SAFETY:`, conforme [09, §4](../../../docs/09-padroes-de-codigo.md)),
//! e depende só de `ir-proto` para os tipos que viajam. Ele não conhece a sessão nem a rede.

#![cfg_attr(
    test,
    allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::indexing_slicing,
        clippy::panic
    )
)]

use std::sync::mpsc::Sender;

use ir_proto::input::{Button, HidUsage, PointerPosition, WheelDelta};

pub mod error;
mod pendentes;
pub use error::{InputError, Result};

#[cfg(target_os = "linux")]
mod linux;
#[cfg(windows)]
mod windows;

/// Um evento a injetar na máquina local. Espelha `ir_session::Injection`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum InjectEvent {
    /// Uma tecla física, por HID Usage.
    Key {
        /// Qual.
        usage: HidUsage,
        /// `true` para pressionar.
        pressed: bool,
    },
    /// Um botão do ponteiro.
    Button {
        /// Qual.
        button: Button,
        /// `true` para pressionar.
        pressed: bool,
    },
    /// Movimento de roda.
    Wheel(WheelDelta),
    /// O ponteiro deve ir para esta posição, sempre absoluta
    /// ([05, §4.2](../../../docs/05-windows.md)).
    Pointer(PointerPosition),
}

/// Um evento capturado da entrada local.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum CaptureEvent {
    /// O ponteiro se moveu, em deltas relativos. Enviado enquanto o controle está no par.
    PointerMotion {
        /// Deslocamento horizontal.
        dx: i32,
        /// Deslocamento vertical.
        dy: i32,
    },
    /// O ponteiro está nesta posição absoluta de tela. Enviado enquanto o controle é local, para
    /// a sessão saber a posição real do cursor e detectar a travessia no ponto certo.
    PointerAbsolute {
        /// Posição horizontal, em pixels de tela.
        x: i32,
        /// Posição vertical, em pixels de tela.
        y: i32,
    },
    /// A roda girou.
    Wheel(WheelDelta),
    /// Uma tecla mudou de estado.
    Key {
        /// Qual.
        usage: HidUsage,
        /// `true` para pressionada.
        pressed: bool,
    },
    /// Um botão mudou de estado.
    Button {
        /// Qual.
        button: Button,
        /// `true` para pressionado.
        pressed: bool,
    },
}

/// Injeta entrada na máquina local.
pub trait Injector: Send {
    /// Injeta um evento.
    ///
    /// # Errors
    ///
    /// [`InputError`] se o sistema recusar a injeção — o sintoma do endurecimento de janeiro de
    /// 2026 no Windows quando as origens confiáveis não são atendidas
    /// ([05, §4.4](../../../docs/05-windows.md)).
    fn inject(&mut self, event: InjectEvent) -> Result<()>;

    /// Solta tudo que possa estar pressionado, agora.
    ///
    /// # Errors
    ///
    /// [`InputError`] em falha do sistema.
    fn release_all(&mut self) -> Result<()>;

    /// O desktop em que o último evento foi injetado, onde a plataforma tem mais de um.
    ///
    /// No Windows, `Default`, `Winlogon` (tela de bloqueio e UAC) ou `Screen-saver`. O agente o
    /// conta ao serviço quando muda.
    fn desktop(&self) -> Option<String> {
        None
    }

    /// Os desktops em que este injetor alcança — para o serviço saber o nível desta máquina.
    fn desktops(&self) -> Vec<String> {
        Vec::new()
    }

    /// Se pode injetar fora da área de trabalho: tela de bloqueio, UAC, protetor de tela.
    ///
    /// Onde a plataforma não separa desktops, não há o que permitir.
    fn permitir_desktop_protegido(&mut self, _permitir: bool) {}
}

/// Captura entrada local e suprime a entrada enquanto o controle está no par.
pub trait Capturer: Send {
    /// Liga ou desliga a supressão da entrada local.
    ///
    /// Ligada, o teclado e o mouse desta máquina param de afetá-la e só alimentam a sessão.
    fn set_suppress(&self, on: bool);

    /// Põe o ponteiro local nesta posição absoluta de tela.
    ///
    /// Usado para prender o cursor no ponto de saída enquanto o controle está no par, e para
    /// devolvê-lo na borda certa ao voltar ([05, §5.2](../../../docs/05-windows.md)).
    fn warp_pointer(&self, x: i32, y: i32);
}

/// O tamanho da tela primária em pixels, quando a plataforma sabe informar.
///
/// No Windows vem de `GetSystemMetrics`. No Linux devolve `None` — o serviço usa o valor da
/// configuração —, porque obter isso sem sessão gráfica não é confiável.
#[must_use]
pub fn primary_screen_size() -> Option<(u32, u32)> {
    #[cfg(windows)]
    {
        windows::primary_screen_size()
    }
    #[cfg(not(windows))]
    {
        None
    }
}

/// Abre o injetor desta plataforma.
///
/// # Errors
///
/// [`InputError::Unsupported`] onde não há backend; [`InputError`] em falha ao abrir o
/// dispositivo (por exemplo sem acesso a `/dev/uinput`).
pub fn open_injector() -> Result<Box<dyn Injector>> {
    #[cfg(target_os = "linux")]
    {
        Ok(Box::new(linux::uinput::UinputInjector::open()?))
    }
    #[cfg(windows)]
    {
        // Uma thread por desktop, para a tela de bloqueio e o UAC (ADR-0008). Se nem a área de
        // trabalho abrir por ela, o injetor simples da thread corrente.
        match windows::desktops::InjetorPorDesktop::novo() {
            Ok(injetor) => Ok(Box::new(injetor)),
            Err(_) => Ok(Box::new(windows::sendinput::SendInputInjector::new())),
        }
    }
    #[cfg(not(any(target_os = "linux", windows)))]
    {
        Err(InputError::Unsupported)
    }
}

/// O arranjo de telas desta sessão — todos os monitores —, onde a plataforma diz.
///
/// No Windows vem de `EnumDisplayMonitors`. Fora dele, `None`: o serviço usa a tela da configuração.
#[must_use]
pub fn arranjo_de_telas() -> Option<ir_proto::screens::ScreenLayout> {
    #[cfg(windows)]
    {
        windows::telas::arranjo()
    }
    #[cfg(not(windows))]
    {
        None
    }
}

/// O retângulo do desktop virtual — origem, largura e altura —, onde a plataforma diz.
#[must_use]
pub fn desktop_virtual() -> Option<(i32, i32, u32, u32)> {
    #[cfg(windows)]
    {
        windows::telas::desktop_virtual()
    }
    #[cfg(not(windows))]
    {
        None
    }
}

/// Bloqueia a tela desta sessão, como o Win+L. Só no Windows, e só de dentro da sessão — é o
/// agente quem chama. Devolve se o sistema aceitou.
#[must_use]
pub fn bloquear_a_tela() -> bool {
    #[cfg(windows)]
    {
        #[allow(unsafe_code)]
        // SAFETY: a função não recebe nada; fora de uma sessão interativa ela só falha.
        unsafe { ::windows::Win32::System::Shutdown::LockWorkStation() }.is_ok()
    }
    #[cfg(not(windows))]
    {
        false
    }
}

/// O desktop que recebe a entrada agora — `Default`, `Winlogon`, `Screen-saver` — no Windows.
///
/// Fora do Windows não há desktops separados, e a resposta é sempre `None`.
#[must_use]
pub fn desktop_de_entrada() -> Option<String> {
    #[cfg(windows)]
    {
        windows::desktops::nome_do_desktop_de_entrada()
    }
    #[cfg(not(windows))]
    {
        None
    }
}

/// Se esta plataforma sabe capturar o teclado e o mouse locais — isto é, se esta máquina pode ser
/// a que tem o teclado.
///
/// É o mesmo fato que [`start_capture`] expressa ao falhar com [`InputError::Unsupported`], dito
/// **antes** de tentar: quem decide o papel da máquina precisa saber disso sem instalar ganchos
/// para descobrir. No Windows, ganchos de baixo nível no agente; no Linux, `evdev` lido pelo
/// serviço, que roda como root ([`linux::captura`]).
#[must_use]
pub const fn capture_supported() -> bool {
    cfg!(any(windows, target_os = "linux"))
}

/// Começa a capturar, entregando os eventos por `sink`.
///
/// # Errors
///
/// [`InputError::Unsupported`] onde não há backend de captura, ou no Linux sem dispositivo legível;
/// [`InputError`] em falha ao instalar os ganchos.
pub fn start_capture(sink: Sender<CaptureEvent>) -> Result<Box<dyn Capturer>> {
    #[cfg(windows)]
    {
        Ok(Box::new(windows::hooks::HookCapturer::start(sink)?))
    }
    #[cfg(target_os = "linux")]
    {
        Ok(Box::new(linux::captura::EvdevCapturer::start(sink)?))
    }
    #[cfg(not(any(windows, target_os = "linux")))]
    {
        // O canal é descartado explicitamente: quem chamou entregou a ponta de escrita, e
        // largá-la fecha o canal na hora, em vez de deixar quem escuta esperando para sempre.
        drop(sink);
        Err(InputError::Unsupported)
    }
}

// Só onde a captura não existe: no Windows e no Linux, `start_capture` toma o teclado de verdade
// de quem roda o teste, e um teste de unidade não tem o direito de fazer isso.
#[cfg(all(test, not(windows), not(target_os = "linux")))]
mod tests {
    use super::*;

    #[test]
    fn unsupported_capture_is_declared_before_trying() {
        // As duas respostas precisam concordar: quem decide o papel da máquina pergunta a
        // `capture_supported`, e quem liga a entrada chama `start_capture`. Se divergirem, o
        // serviço aceita um papel que depois não consegue exercer.
        let (sink, _recebidos) = std::sync::mpsc::channel();
        assert!(!capture_supported());
        assert!(matches!(start_capture(sink), Err(InputError::Unsupported)));
    }
}
