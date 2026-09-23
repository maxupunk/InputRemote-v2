//! A economia de energia do Wi-Fi desta máquina: ver se a placa cochila entre pacotes, e desligar.
//!
//! Com a economia ligada, a placa dorme entre pacotes e o ponto de acesso guarda o que chega até o
//! próximo *beacon* — até ~100 ms, às vezes mais. Os comandos de entrada são pequenos e frequentes,
//! e o driver não os conta como tráfego que valha acordar: o ponteiro pela rede flui, trava e volta
//! a fluir. Na bancada, o ping para um Fedora com a economia ligada foi a 190 ms; desligada, o pior
//! caso caiu para 12 ms ([log 44](../../../docs/logs/44-o-wifi-que-cochilava.md)).
//!
//! Este crate só **lê** e **desliga**, e só a economia do Wi-Fi. Quem decide quando perguntar é o
//! serviço; quem pede para desligar é o usuário, pelo botão da janela.
//!
//! As funções bloqueiam (rodam comandos do sistema): chame-as fora do laço do serviço.

#![forbid(unsafe_code)]
#![cfg_attr(
    test,
    allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::indexing_slicing,
        clippy::panic
    )
)]

mod leitura;
#[cfg(target_os = "linux")]
mod linux;
#[cfg(windows)]
mod windows;

pub use leitura::{economia_do_iw, economia_do_powercfg};

/// Como está a economia de energia do Wi-Fi desta máquina.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Economia {
    /// Desligada, ou não há Wi-Fi.
    Desligada,
    /// Ligada agora: a placa cochila entre pacotes.
    Ligada,
    /// Desligada na tomada, ligada na bateria.
    SoNaBateria,
    /// Não deu para saber — falta a ferramenta do sistema, ou ela respondeu algo inesperado.
    Desconhecida,
}

/// Por que não deu para desligar.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ErroDeEnergia(pub String);

impl core::fmt::Display for ErroDeEnergia {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for ErroDeEnergia {}

/// Como está a economia de energia do Wi-Fi agora.
#[must_use]
pub fn verificar() -> Economia {
    #[cfg(target_os = "linux")]
    {
        linux::verificar()
    }
    #[cfg(windows)]
    {
        windows::verificar()
    }
    #[cfg(not(any(target_os = "linux", windows)))]
    {
        Economia::Desconhecida
    }
}

/// Desliga a economia de energia do Wi-Fi — agora, e de um jeito que sobreviva a reconexões.
///
/// Exige o privilégio do serviço: root no Linux, SYSTEM no Windows.
///
/// # Errors
///
/// [`ErroDeEnergia`] com o motivo, quando a ferramenta do sistema falta ou recusa.
pub fn desligar() -> Result<(), ErroDeEnergia> {
    #[cfg(target_os = "linux")]
    {
        linux::desligar()
    }
    #[cfg(windows)]
    {
        windows::desligar()
    }
    #[cfg(not(any(target_os = "linux", windows)))]
    {
        Err(ErroDeEnergia("sistema sem suporte".to_owned()))
    }
}

/// Roda um comando e devolve a saída padrão, ou o motivo de não ter dado.
#[cfg(any(target_os = "linux", windows))]
fn rodar(programa: &str, argumentos: &[&str]) -> Result<String, ErroDeEnergia> {
    let saida = std::process::Command::new(programa)
        .args(argumentos)
        .output()
        .map_err(|erro| ErroDeEnergia(format!("{programa}: {erro}")))?;
    if !saida.status.success() {
        let motivo = String::from_utf8_lossy(&saida.stderr);
        return Err(ErroDeEnergia(format!(
            "{programa} {}: {}",
            argumentos.join(" "),
            motivo.trim()
        )));
    }
    Ok(String::from_utf8_lossy(&saida.stdout).into_owned())
}
