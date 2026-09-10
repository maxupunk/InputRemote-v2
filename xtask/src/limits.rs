//! Os limites de tamanho de `docs/09-padroes-de-codigo.md` §1.
//!
//! Cada número aqui existe porque o InputRemote 1 o violou. `controller.rs` tinha 4.545
//! linhas — onze vezes o limite de arquivo. Nenhum commit isolado criou aquele arquivo; ele
//! cresceu porque nada o impedia.
//!
//! Estourar um limite é sinal de que falta uma abstração, não de que o limite está errado.

use crate::scan::{RustFile, Violation};

/// Máximo de linhas num arquivo `.rs`.
pub(crate) const MAX_FILE_LINES: usize = 400;

/// Máximo de linhas numa função.
pub(crate) const MAX_FN_LINES: usize = 60;

/// Máximo de linhas de **produção** num crate, somando `src/`.
///
/// Linhas de teste não contam: o limite existe para conter responsabilidade, e teste não
/// acrescenta responsabilidade. Contá-lo empurraria a testar menos.
pub(crate) const MAX_CRATE_LINES: usize = 2_500;

/// Verifica todos os limites e devolve o que foi violado.
pub(crate) fn check(files: &[RustFile]) -> Vec<Violation> {
    let mut violations = Vec::new();

    for file in files {
        if file.lines > MAX_FILE_LINES {
            violations.push(Violation {
                path: file.path.clone(),
                line: file.lines,
                message: format!(
                    "arquivo com {} linhas, limite {MAX_FILE_LINES}. Falta uma abstração — \
                     divida por responsabilidade, não aumente o limite.",
                    file.lines
                ),
            });
        }
        violations.extend(long_functions(file));
    }

    violations.extend(large_crates(files));
    violations
}

/// Funções acima do limite de linhas.
///
/// A contagem é da chave de abertura até a de fechamento, no mesmo nível de indentação da
/// assinatura. É uma heurística, não um analisador sintático: erra em macros que abrem chaves
/// sem fechar na mesma linha, e acerta em todo o resto. Para o propósito — impedir que uma
/// função cresça sem ninguém notar — é suficiente, e não custa uma dependência de `syn`.
fn long_functions(file: &RustFile) -> Vec<Violation> {
    let mut violations = Vec::new();
    // Nome, linha da assinatura, e quantas chaves ainda estão abertas.
    let mut current: Option<(String, usize, i32)> = None;

    for (index, line) in file.text.lines().enumerate() {
        let number = index + 1;
        let delta = brace_delta(line);

        match current.take() {
            None => {
                let trimmed = line.trim_start();
                if is_fn_signature(trimmed) && delta > 0 {
                    let name = function_name(trimmed).unwrap_or("<anônima>").to_owned();
                    current = Some((name, number, delta));
                }
                // Assinatura com o corpo na mesma linha cabe no limite por definição.
            }
            Some((name, start, depth)) => {
                let depth = depth + delta;
                if depth > 0 {
                    current = Some((name, start, depth));
                    continue;
                }
                let length = number - start + 1;
                if length > MAX_FN_LINES {
                    violations.push(Violation {
                        path: file.path.clone(),
                        line: start,
                        message: format!(
                            "função `{name}` com {length} linhas, limite {MAX_FN_LINES}.                              Uma função longa esconde o que faz; extraia as partes."
                        ),
                    });
                }
            }
        }
    }

    violations
}

/// Quantas chaves a linha abre a mais do que fecha. Pode ser negativo.
///
/// Contar com sinal é o que faz a profundidade voltar a zero. Uma versão que saturasse em
/// zero nunca fecharia função nenhuma — foi o defeito que o teste
/// `every_flavour_of_signature_is_recognised` pegou.
fn brace_delta(line: &str) -> i32 {
    let code = strip_line_comment(line);
    let opens = i32::try_from(code.matches('{').count()).unwrap_or(i32::MAX);
    let closes = i32::try_from(code.matches('}').count()).unwrap_or(i32::MAX);
    opens - closes
}

fn is_fn_signature(trimmed: &str) -> bool {
    let without_attrs = trimmed
        .trim_start_matches("pub(crate) ")
        .trim_start_matches("pub ");
    let without_attrs = without_attrs
        .trim_start_matches("pub(super) ")
        .trim_start_matches("const ")
        .trim_start_matches("async ")
        .trim_start_matches("unsafe ")
        .trim_start_matches("extern ");
    without_attrs.starts_with("fn ")
}

fn function_name(trimmed: &str) -> Option<&str> {
    let after = trimmed.split("fn ").nth(1)?;
    let end = after.find(['(', '<', ' ']).unwrap_or(after.len());
    after.get(..end)
}

fn strip_line_comment(line: &str) -> &str {
    match line.find("//") {
        Some(at) => line.get(..at).unwrap_or(line),
        None => line,
    }
}

/// Crates acima do limite de linhas em `src/`.
fn large_crates(files: &[RustFile]) -> Vec<Violation> {
    let mut totals: Vec<(String, usize)> = Vec::new();

    for file in files {
        let Some(crate_name) = crate_of(&file.path) else {
            continue;
        };
        if !file.path.contains("/src/") && !file.path.contains("\\src\\") {
            continue;
        }
        match totals.iter_mut().find(|(name, _)| *name == crate_name) {
            Some((_, total)) => *total += file.production_lines,
            None => totals.push((crate_name, file.production_lines)),
        }
    }

    totals
        .into_iter()
        .filter(|(_, total)| *total > MAX_CRATE_LINES)
        .map(|(name, total)| Violation {
            path: format!("crates/{name}"),
            line: total,
            message: format!(
                "crate com {total} linhas de produção em src/, limite {MAX_CRATE_LINES}. \
                 Extraia um crate quando a fronteira estiver comprovada."
            ),
        })
        .collect()
}

/// O nome do crate a que um caminho pertence.
fn crate_of(path: &str) -> Option<String> {
    let normalised = path.replace('\\', "/");
    let after = normalised.split("crates/").nth(1)?;
    let name = after.split('/').next()?;
    Some(name.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(path: &str, text: &str) -> RustFile {
        RustFile {
            path: path.to_owned(),
            lines: text.lines().count(),
            production_lines: text.lines().count(),
            text: text.to_owned(),
        }
    }

    #[test]
    fn a_file_within_the_limit_passes() {
        let text = "fn small() {\n    let _ = 1;\n}\n";
        assert!(check(&[file("crates/ir-proto/src/x.rs", text)]).is_empty());
    }

    #[test]
    fn an_oversized_file_is_caught() {
        let text = "// linha\n".repeat(MAX_FILE_LINES + 1);
        let found = check(&[file("crates/ir-proto/src/x.rs", &text)]);
        assert_eq!(found.len(), 1);
        assert!(
            found[0].message.contains("Falta uma abstração"),
            "a saída precisa estar dita"
        );
    }

    #[test]
    fn a_file_exactly_at_the_limit_passes() {
        let text = "// linha\n".repeat(MAX_FILE_LINES);
        assert!(check(&[file("crates/ir-proto/src/x.rs", &text)]).is_empty());
    }

    #[test]
    fn a_long_function_is_caught_with_its_name() {
        let body = "    let _ = 1;\n".repeat(MAX_FN_LINES + 5);
        let text = format!("fn gorda() {{\n{body}}}\n");
        let found = check(&[file("crates/ir-proto/src/x.rs", &text)]);
        assert_eq!(found.len(), 1);
        assert!(
            found[0].message.contains("gorda"),
            "a mensagem precisa dizer qual função"
        );
    }

    #[test]
    fn a_short_function_passes() {
        let text = "fn magra() {\n    let _ = 1;\n}\n";
        assert!(check(&[file("crates/ir-proto/src/x.rs", text)]).is_empty());
    }

    #[test]
    fn every_flavour_of_signature_is_recognised() {
        for prefix in [
            "fn ",
            "pub fn ",
            "pub(crate) fn ",
            "const fn ",
            "pub const fn ",
            "async fn ",
        ] {
            let body = "    let _ = 1;\n".repeat(MAX_FN_LINES + 5);
            let text = format!("{prefix}alvo() {{\n{body}}}\n");
            let found = check(&[file("crates/ir-proto/src/x.rs", &text)]);
            assert_eq!(
                found.len(),
                1,
                "`{prefix}` não foi reconhecido como assinatura"
            );
        }
    }

    #[test]
    fn a_brace_in_a_comment_does_not_confuse_the_counter() {
        let text = "fn alvo() {\n    // uma chave } num comentário\n    let _ = 1;\n}\n";
        assert!(check(&[file("crates/ir-proto/src/x.rs", text)]).is_empty());
    }

    #[test]
    fn an_oversized_crate_is_caught_by_summing_its_files() {
        let chunk = "let _ = 1;\n".repeat(MAX_CRATE_LINES / 2 + 10);
        let files = vec![
            file("crates/ir-proto/src/a.rs", &chunk),
            file("crates/ir-proto/src/b.rs", &chunk),
        ];
        let found: Vec<_> = check(&files)
            .into_iter()
            .filter(|violation| violation.path == "crates/ir-proto")
            .collect();
        assert_eq!(found.len(), 1);
    }

    #[test]
    fn files_outside_src_do_not_count_towards_the_crate_limit() {
        let chunk = "let _ = 1;\n".repeat(MAX_CRATE_LINES + 10);
        let found: Vec<_> = check(&[file("crates/ir-proto/tests/big.rs", &chunk)])
            .into_iter()
            .filter(|violation| violation.path == "crates/ir-proto")
            .collect();
        assert!(
            found.is_empty(),
            "teste não conta no limite de complexidade do crate"
        );
    }
}
