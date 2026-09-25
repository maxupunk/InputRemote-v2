//! Rodar uma ferramenta do sistema (`reg.exe`, por exemplo) sem deixar quem chama preso a ela.
//!
//! Uma ferramenta que trava — um registro bloqueado, um driver que não responde — prenderia para
//! sempre a thread que a chamou. `Command::output()` não tem prazo; aqui há. É o corredor do
//! `reg.exe` da política de atenção (`ir-sessao`) e do `iw`/`netsh`/`powercfg` da economia de
//! energia (`ir-energia`).

use std::io::Read;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};

/// Roda `programa` com `argumentos` e devolve a saída padrão — no máximo em `prazo`.
///
/// # Errors
///
/// Se o programa não abrir, não responder no prazo (e aí é encerrado), ou sair com falha — com o
/// que ele escreveu no erro padrão.
pub fn rodar_com_prazo(programa: &str, argumentos: &[&str], prazo: Duration) -> Result<String> {
    let mut filho = Command::new(programa)
        .args(argumentos)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .with_context(|| format!("abrindo {programa}"))?;
    // As saídas são lidas em threads próprias: um filho que escreve mais do que cabe no cano
    // travaria esperando alguém ler, e a espera abaixo nunca terminaria.
    let saida = ler_em_fundo(
        filho
            .stdout
            .take()
            .map(|c| Box::new(c) as Box<dyn Read + Send>),
    );
    let erros = ler_em_fundo(
        filho
            .stderr
            .take()
            .map(|c| Box::new(c) as Box<dyn Read + Send>),
    );
    let limite = Instant::now() + prazo;
    let status = loop {
        if let Some(status) = filho
            .try_wait()
            .with_context(|| format!("esperando {programa}"))?
        {
            break status;
        }
        if Instant::now() >= limite {
            let _ = filho.kill();
            let _ = filho.wait();
            bail!("{programa} não respondeu em {} s", prazo.as_secs());
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    let saida = saida.join().unwrap_or_default();
    let erros = erros.join().unwrap_or_default();
    if !status.success() {
        let motivo = String::from_utf8_lossy(&erros);
        bail!("{programa} {}: {}", argumentos.join(" "), motivo.trim());
    }
    Ok(String::from_utf8_lossy(&saida).into_owned())
}

/// Lê um cano até o fim numa thread própria.
fn ler_em_fundo(cano: Option<Box<dyn Read + Send>>) -> std::thread::JoinHandle<Vec<u8>> {
    std::thread::spawn(move || {
        let mut texto = Vec::new();
        if let Some(mut cano) = cano {
            let _ = cano.read_to_end(&mut texto);
        }
        texto
    })
}

/// Os números em hexadecimal (`0x…`) de uma saída, na ordem.
///
/// As ferramentas do Windows mudam de idioma com o sistema: o texto em volta não se lê, e o número
/// em hexadecimal é o mesmo em qualquer idioma.
pub fn numeros_hex(saida: &str) -> impl Iterator<Item = u32> + '_ {
    saida
        .split_whitespace()
        .filter_map(|palavra| palavra.strip_prefix("0x"))
        .filter_map(|hex| u32::from_str_radix(hex, 16).ok())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    #[test]
    fn o_hexadecimal_e_lido_em_qualquer_idioma() {
        let saida = "Índice atual: 0x00000000\nCurrent DC Power Setting Index: 0x00000002\n";
        assert_eq!(numeros_hex(saida).collect::<Vec<_>>(), [0, 2]);
        assert_eq!(numeros_hex("nada aqui").next(), None);
    }

    #[cfg(any(windows, target_os = "linux"))]
    #[test]
    fn a_ferramenta_que_trava_e_abandonada_no_prazo() {
        #[cfg(windows)]
        let (programa, argumentos) = ("ping", ["-n", "30", "127.0.0.1"].as_slice());
        #[cfg(target_os = "linux")]
        let (programa, argumentos) = ("sleep", ["30"].as_slice());
        let inicio = Instant::now();
        let erro = rodar_com_prazo(programa, argumentos, Duration::from_millis(300)).unwrap_err();
        assert!(erro.to_string().contains("não respondeu"), "{erro}");
        assert!(inicio.elapsed() < Duration::from_secs(5));
    }

    #[cfg(any(windows, target_os = "linux"))]
    #[test]
    fn a_ferramenta_rapida_devolve_a_saida() {
        #[cfg(windows)]
        let saida = rodar_com_prazo("cmd", &["/C", "echo", "oi"], Duration::from_secs(5)).unwrap();
        #[cfg(target_os = "linux")]
        let saida = rodar_com_prazo("echo", &["oi"], Duration::from_secs(5)).unwrap();
        assert_eq!(saida.trim(), "oi");
    }
}
