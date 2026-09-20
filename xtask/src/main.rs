//! As verificações de `docs/09-padroes-de-codigo.md`, executáveis.
//!
//! Uma regra que só existe no documento é uma regra que já foi quebrada. Estas rodam no CI, e
//! falham a build — não avisam. Aviso é ruído que se aprende a ignorar, e foi assim que o
//! `ROADMAP.md` do InputRemote 1 chegou ao lançamento com itens em aberto.
//!
//! ```text
//! cargo xtask check          tudo
//! cargo xtask check-limits   tamanho de arquivo, função e crate
//! cargo xtask check-deps     as setas de docs/02 §2, e a pureza do núcleo
//! cargo xtask check-logs     nenhum log com conteúdo digitado
//! cargo xtask check-texto    nenhum texto em UTF-8 codificado duas vezes
//! ```

#![cfg_attr(
    test,
    allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::indexing_slicing,
        clippy::panic
    )
)]

mod deps;
mod limits;
mod logs;
mod scan;
mod texto;

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Result, bail};

use scan::Violation;

fn main() -> ExitCode {
    match run() {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(error) => {
            eprintln!("xtask falhou: {error:#}");
            ExitCode::FAILURE
        }
    }
}

/// Roda o que foi pedido. `Ok(false)` significa que houve violação.
fn run() -> Result<bool> {
    let task = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "check".to_owned());
    let root = repository_root()?;

    let checks: Vec<Check> = match task.as_str() {
        "check" => vec![Check::Limits, Check::Deps, Check::Logs, Check::Texto],
        "check-limits" => vec![Check::Limits],
        "check-deps" => vec![Check::Deps],
        "check-logs" => vec![Check::Logs],
        "check-texto" => vec![Check::Texto],
        other => bail!(
            "tarefa desconhecida: `{other}`. \
             Use `check`, `check-limits`, `check-deps`, `check-logs` ou `check-texto`."
        ),
    };

    let files = scan::rust_files(&root)?;
    let mut violations = Vec::new();

    for check in checks {
        let found = match check {
            Check::Limits => limits::check(&files),
            Check::Deps => deps::check(&root)?,
            Check::Logs => logs::check(&files),
            Check::Texto => texto::check(&files),
        };
        report(check.name(), &found);
        violations.extend(found);
    }

    if violations.is_empty() {
        println!(
            "\ntudo dentro das regras: {} arquivos verificados",
            files.len()
        );
        return Ok(true);
    }

    println!(
        "\n{} violações. A build falha até que sejam resolvidas.",
        violations.len()
    );
    Ok(false)
}

#[derive(Debug, Clone, Copy)]
enum Check {
    Limits,
    Deps,
    Logs,
    Texto,
}

impl Check {
    const fn name(self) -> &'static str {
        match self {
            Self::Limits => "limites de tamanho",
            Self::Deps => "setas de dependência e pureza",
            Self::Logs => "privacidade dos logs",
            Self::Texto => "texto sem dupla codificação",
        }
    }
}

fn report(name: &str, violations: &[Violation]) {
    if violations.is_empty() {
        println!("ok  {name}");
        return;
    }
    println!("FALHA  {name}");
    for violation in violations {
        println!("       {violation}");
    }
}

/// A raiz do repositório: onde está o `Cargo.toml` do workspace.
///
/// Procura subindo a partir do diretório do próprio manifesto do `xtask`, de forma que
/// `cargo xtask` funcione de qualquer subdiretório.
fn repository_root() -> Result<PathBuf> {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let mut current: &Path = &manifest;

    loop {
        if is_workspace_root(current) {
            return Ok(current.to_path_buf());
        }
        match current.parent() {
            Some(parent) => current = parent,
            None => bail!(
                "não achei a raiz do workspace a partir de {}",
                manifest.display()
            ),
        }
    }
}

fn is_workspace_root(dir: &Path) -> bool {
    let manifest = dir.join("Cargo.toml");
    let Ok(text) = std::fs::read_to_string(&manifest) else {
        return false;
    };
    text.contains("[workspace]")
}
