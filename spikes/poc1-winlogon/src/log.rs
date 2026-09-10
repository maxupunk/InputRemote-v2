//! Registro em arquivo, porque um serviço não tem console para onde escrever.
//!
//! Deliberadamente cru: sem `tracing`, sem dependência, sem estrutura. O que interessa é que
//! sobreviva ao serviço morrer e possa ser lido depois do desbloqueio.

use std::fmt::Write as _;
use std::fs::{self, OpenOptions};
use std::io::Write as _;
use std::path::PathBuf;

/// Onde o registro fica.
///
/// `%ProgramData%` e não `%TEMP%`: o serviço roda como `SYSTEM`, cujo `%TEMP%` fica dentro de
/// `C:\Windows`, e um arquivo lá é mais difícil de achar do que de escrever.
pub fn path() -> PathBuf {
    let base = std::env::var("ProgramData").unwrap_or_else(|_| "C:\\ProgramData".to_owned());
    PathBuf::from(base).join("poc1")
}

/// Escreve uma linha, com carimbo de tempo em milissegundos desde o início do processo.
pub fn line(message: &str) {
    let dir = path();
    let _ = fs::create_dir_all(&dir);
    let file = dir.join("poc1.log");

    let mut text = String::new();
    let _ = write!(text, "[{:>9} ms] {message}", elapsed_millis());
    text.push_str("\r\n");

    if let Ok(mut handle) = OpenOptions::new().create(true).append(true).open(&file) {
        let _ = handle.write_all(text.as_bytes());
    }
}

/// Milissegundos desde a primeira chamada, para medir a detecção de troca de desktop.
fn elapsed_millis() -> u128 {
    use std::sync::OnceLock;
    use std::time::Instant;

    static START: OnceLock<Instant> = OnceLock::new();
    START.get_or_init(Instant::now).elapsed().as_millis()
}

/// Escreve o cabeçalho do registro, com o que precisa constar no resultado.
pub fn header(role: &str) {
    line("");
    line(&format!("=== {role} ==="));
    line(&format!("modo: {}", super::mode()));
    if let Ok(build) = build_number() {
        line(&format!("build do Windows: {build}"));
        line("        (o resultado só vale em build >= 26100.7623 / 26200.7623 / 22631.6491)");
    }
}

/// O número de build, lido do registro.
fn build_number() -> Result<String, std::io::Error> {
    use std::process::Command;

    let output = Command::new("reg")
        .args([
            "query",
            r"HKLM\SOFTWARE\Microsoft\Windows NT\CurrentVersion",
            "/v",
            "CurrentBuild",
        ])
        .output()?;

    let text = String::from_utf8_lossy(&output.stdout);
    let build = text
        .lines()
        .find_map(|line| line.split_whitespace().last().map(str::to_owned))
        .unwrap_or_else(|| "desconhecido".to_owned());
    Ok(build)
}
