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

/// Quanto uma ferramenta do sistema pode demorar antes de ser abandonada.
///
/// As verificações rodam numa thread de bloqueio do serviço, a cada 30 s e a pedido do par. Uma
/// ferramenta que trava — um driver que não responde ao `iw` — prenderia essa thread para sempre,
/// e a próxima verificação prenderia outra.
#[cfg(any(target_os = "linux", windows))]
const PRAZO_DA_FERRAMENTA: std::time::Duration = std::time::Duration::from_secs(5);

/// Roda um comando e devolve a saída padrão, ou o motivo de não ter dado — no máximo em
/// [`PRAZO_DA_FERRAMENTA`].
#[cfg(any(target_os = "linux", windows))]
fn rodar(programa: &str, argumentos: &[&str]) -> Result<String, ErroDeEnergia> {
    rodar_com_prazo(programa, argumentos, PRAZO_DA_FERRAMENTA)
}

#[cfg(any(target_os = "linux", windows))]
fn rodar_com_prazo(
    programa: &str,
    argumentos: &[&str],
    prazo: std::time::Duration,
) -> Result<String, ErroDeEnergia> {
    use std::io::Read;
    use std::process::Stdio;

    let falhou = |erro: &dyn std::fmt::Display| ErroDeEnergia(format!("{programa}: {erro}"));
    let mut filho = std::process::Command::new(programa)
        .args(argumentos)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|erro| falhou(&erro))?;
    // As saídas são lidas em threads próprias: um filho que escreve mais do que cabe no cano
    // travaria esperando alguém ler, e a espera abaixo nunca terminaria.
    let ler = |cano: Option<Box<dyn Read + Send>>| {
        std::thread::spawn(move || {
            let mut texto = Vec::new();
            if let Some(mut cano) = cano {
                let _ = cano.read_to_end(&mut texto);
            }
            texto
        })
    };
    let saida = ler(filho
        .stdout
        .take()
        .map(|c| Box::new(c) as Box<dyn Read + Send>));
    let erros = ler(filho
        .stderr
        .take()
        .map(|c| Box::new(c) as Box<dyn Read + Send>));
    let limite = std::time::Instant::now() + prazo;
    let status = loop {
        if let Some(status) = filho.try_wait().map_err(|erro| falhou(&erro))? {
            break status;
        }
        if std::time::Instant::now() >= limite {
            let _ = filho.kill();
            let _ = filho.wait();
            return Err(ErroDeEnergia(format!(
                "{programa} não respondeu em {} s",
                prazo.as_secs()
            )));
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    };
    let saida = saida.join().unwrap_or_default();
    let erros = erros.join().unwrap_or_default();
    if !status.success() {
        let motivo = String::from_utf8_lossy(&erros);
        return Err(ErroDeEnergia(format!(
            "{programa} {}: {}",
            argumentos.join(" "),
            motivo.trim()
        )));
    }
    Ok(String::from_utf8_lossy(&saida).into_owned())
}

#[cfg(all(test, any(target_os = "linux", windows)))]
mod tests_do_prazo {
    use super::*;

    #[test]
    fn a_ferramenta_que_trava_e_abandonada_no_prazo() {
        #[cfg(windows)]
        let (programa, argumentos) = ("ping", ["-n", "30", "127.0.0.1"]);
        #[cfg(target_os = "linux")]
        let (programa, argumentos) = ("sleep", ["30", ""]);
        let argumentos: Vec<&str> = argumentos.into_iter().filter(|a| !a.is_empty()).collect();
        let inicio = std::time::Instant::now();
        let erro = rodar_com_prazo(programa, &argumentos, std::time::Duration::from_millis(300))
            .unwrap_err();
        assert!(erro.0.contains("não respondeu"), "{}", erro.0);
        assert!(inicio.elapsed() < std::time::Duration::from_secs(5));
    }

    #[test]
    fn a_ferramenta_rapida_devolve_a_saida() {
        #[cfg(windows)]
        let saida = rodar("cmd", &["/C", "echo", "oi"]).unwrap();
        #[cfg(target_os = "linux")]
        let saida = rodar("echo", &["oi"]).unwrap();
        assert_eq!(saida.trim(), "oi");
    }
}
