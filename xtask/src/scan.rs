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
    zerar_modulos_de_teste(&mut found);
    Ok(found)
}

/// Tira da contagem de produção os módulos de teste que moram em arquivo próprio.
///
/// `count_production_lines` só enxerga `mod tests { ... }` dentro do arquivo. Um
/// `#[cfg(test)] mod bancada;` põe o teste noutro arquivo, que o compilador nem vê fora de
/// teste — e que era contado inteiro como produção, contra a regra que o próprio limite declara.
/// Os filhos do módulo (a pasta dele) saem junto.
fn zerar_modulos_de_teste(files: &mut [RustFile]) {
    let raizes: Vec<String> = files.iter().flat_map(modulos_de_teste).collect();
    for file in files.iter_mut() {
        let de_teste = raizes.iter().any(|raiz| {
            file.path == format!("{raiz}.rs") || file.path.starts_with(&format!("{raiz}/"))
        });
        if de_teste {
            file.production_lines = 0;
        }
    }
}

/// Os caminhos (sem extensão) dos módulos que este arquivo declara só para teste.
fn modulos_de_teste(file: &RustFile) -> Vec<String> {
    let pasta = match file.path.rsplit_once('/') {
        Some((pasta, "mod.rs" | "lib.rs" | "main.rs")) => pasta.to_owned(),
        Some(_) | None => file.path.trim_end_matches(".rs").to_owned(),
    };
    let linhas: Vec<&str> = file
        .text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();
    linhas
        .windows(2)
        .filter_map(|par| match par {
            ["#[cfg(test)]", declaracao] => declaracao_de_modulo(declaracao),
            _ => None,
        })
        .map(|nome| format!("{pasta}/{nome}"))
        .collect()
}

/// `mod x;` ou `pub(crate) mod x;` → `x`. Módulo com corpo (`mod x {`) não é arquivo.
fn declaracao_de_modulo(linha: &str) -> Option<&str> {
    let resto = linha.strip_suffix(';')?;
    let (_, nome) = resto.rsplit_once("mod ")?;
    let nome = nome.trim();
    nome.chars()
        .all(|c| c.is_alphanumeric() || c == '_')
        .then_some(nome)
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

#[cfg(test)]
mod tests {
    use super::*;

    fn arquivo(path: &str, text: &str) -> RustFile {
        RustFile {
            path: path.to_owned(),
            lines: text.lines().count(),
            production_lines: count_production_lines(text),
            text: text.to_owned(),
        }
    }

    #[test]
    fn modulo_de_teste_em_arquivo_proprio_nao_conta_como_producao() {
        let mut files = vec![
            arquivo(
                "crates/a/src/actor/mod.rs",
                "#[cfg(test)]
mod bancada;
mod partes;
fn x() {}
",
            ),
            arquivo(
                "crates/a/src/actor/bancada.rs",
                "fn a() {}
fn b() {}
",
            ),
            arquivo(
                "crates/a/src/actor/bancada/filho.rs",
                "fn c() {}
",
            ),
            arquivo(
                "crates/a/src/actor/partes.rs",
                "fn d() {}
",
            ),
        ];
        zerar_modulos_de_teste(&mut files);
        let producao: Vec<usize> = files.iter().map(|f| f.production_lines).collect();
        assert_eq!(producao, vec![3, 0, 0, 1]);
    }

    #[test]
    fn arquivo_comum_declara_filhos_na_pasta_com_o_proprio_nome() {
        let file = arquivo(
            "crates/a/src/cliente.rs",
            "#[cfg(test)]
pub(crate) mod testes;
",
        );
        assert_eq!(modulos_de_teste(&file), vec!["crates/a/src/cliente/testes"]);
    }

    #[test]
    fn modulo_de_teste_com_corpo_fica_com_a_contagem_interna() {
        let file = arquivo(
            "crates/a/src/lib.rs",
            "#[cfg(test)]
mod coagido_tests {
",
        );
        assert!(modulos_de_teste(&file).is_empty());
    }
}
