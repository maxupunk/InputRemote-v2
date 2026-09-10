//! Leitura dos arquivos do repositório.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

/// Um arquivo `.rs` lido do disco.
#[derive(Debug, Clone)]
pub(crate) struct RustFile {
    /// Caminho relativo à raiz do repositório, com `/` como separador.
    pub(crate) path: String,
    /// O conteúdo.
    pub(crate) text: String,
    /// Quantas linhas tem, incluindo testes.
    pub(crate) lines: usize,
    /// Linhas de **código** de produção: sem testes, sem comentários, sem linhas em branco.
    ///
    /// É esta que conta no limite por crate, e a escolha é deliberada. O limite existe para
    /// conter **complexidade**, e nem teste nem documentação acrescentam complexidade — teste
    /// acrescenta confiança, documentação reduz o custo de entender o que já está lá. Contar
    /// qualquer um dos dois criaria o incentivo de escrever menos deles, que é o oposto do que
    /// este projeto quer.
    ///
    /// O limite por **arquivo**, esse conta tudo: ali o que se protege é a navegabilidade, e
    /// um arquivo longo é longo de rolar mesmo que a maior parte seja documentação.
    pub(crate) production_lines: usize,
}

/// Uma regra violada.
#[derive(Debug, Clone)]
pub(crate) struct Violation {
    /// Onde.
    pub(crate) path: String,
    /// Em qual linha, ou o número que estourou o limite.
    pub(crate) line: usize,
    /// O que está errado, e o que fazer.
    ///
    /// A terceira parte não é opcional: uma mensagem que diz o problema sem dizer a saída
    /// obriga quem a lê a adivinhar (`docs/09-padroes-de-codigo.md` §5).
    pub(crate) message: String,
}

impl std::fmt::Display for Violation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}: {}", self.path, self.line, self.message)
    }
}

/// Todos os arquivos `.rs` do repositório, fora de `target` e de `spikes`.
///
/// `spikes` fica de fora porque é código descartável de prova de conceito
/// (`docs/08-plano-de-implementacao.md` §2): aplicar os limites a ele atrasaria a resposta a
/// perguntas sem melhorar nada.
pub(crate) fn rust_files(root: &Path) -> Result<Vec<RustFile>> {
    let mut found = Vec::new();
    collect(root, root, &mut found)?;
    found.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(found)
}

fn collect(root: &Path, dir: &Path, out: &mut Vec<RustFile>) -> Result<()> {
    let entries = fs::read_dir(dir).with_context(|| format!("lendo {}", dir.display()))?;

    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();

        if path.is_dir() {
            if matches!(name.as_ref(), "target" | ".git" | "spikes" | "dist") {
                continue;
            }
            collect(root, &path, out)?;
            continue;
        }

        if path.extension().is_some_and(|ext| ext == "rs") {
            let text =
                fs::read_to_string(&path).with_context(|| format!("lendo {}", path.display()))?;
            let relative = path.strip_prefix(root).unwrap_or(&path);
            out.push(RustFile {
                path: relative.to_string_lossy().replace('\\', "/"),
                lines: text.lines().count(),
                production_lines: count_production_lines(&text),
                text,
            });
        }
    }

    Ok(())
}

/// Linhas de código de produção: sem testes, sem comentários, sem linhas em branco.
///
/// A exclusão dos módulos de teste conta chaves a partir da declaração até ela zerar. É
/// heurística, como a contagem de funções de `limits`, e erra para mais — o que faz um crate
/// parecer maior, que é o lado seguro de errar.
fn count_production_lines(text: &str) -> usize {
    let mut total = 0;
    let mut depth = 0usize;
    let mut inside_tests = false;

    for line in text.lines() {
        let trimmed = line.trim();

        if !inside_tests && trimmed.starts_with("mod tests") {
            inside_tests = true;
            depth = 0;
        }
        if inside_tests {
            depth += line.matches('{').count();
            depth = depth.saturating_sub(line.matches('}').count());
            if depth == 0 && line.contains('}') {
                inside_tests = false;
            }
            continue;
        }

        let is_noise =
            trimmed.is_empty() || trimmed.starts_with("//") || trimmed.starts_with("#[cfg(test)]");
        if !is_noise {
            total += 1;
        }
    }

    total
}

/// Todos os `Cargo.toml` de crate do workspace.
pub(crate) fn manifests(root: &Path) -> Result<Vec<(String, PathBuf)>> {
    let crates_dir = root.join("crates");
    if !crates_dir.is_dir() {
        return Ok(Vec::new());
    }

    let mut found = Vec::new();
    for entry in fs::read_dir(&crates_dir)? {
        let entry = entry?;
        let manifest = entry.path().join("Cargo.toml");
        if manifest.is_file() {
            found.push((entry.file_name().to_string_lossy().into_owned(), manifest));
        }
    }
    found.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(found)
}
