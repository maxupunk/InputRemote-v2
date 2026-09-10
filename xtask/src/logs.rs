//! A regra de privacidade dos logs, de `docs/04-seguranca.md` §7.
//!
//! O produto vê tudo que é digitado, inclusive senhas — é a razão de ele existir. Um log que
//! registre uma tecla é um arquivo de texto com a senha do usuário dentro, escrito por um
//! serviço `SYSTEM`, num caminho que sobrevive a reinício.
//!
//! Esta verificação procura macros de log que recebam um valor de tipo de entrada. Não é
//! prova — nenhuma varredura de texto é —, mas pega o caso comum: alguém acrescenta
//! `?usage` a um `tracing::debug!` para depurar e esquece de tirar.

use crate::scan::{RustFile, Violation};

/// Macros de log que a regra cobre.
const LOG_MACROS: &[&str] = &["trace!", "debug!", "info!", "warn!", "error!", "event!"];

/// Nomes de variável e de tipo que denunciam conteúdo digitado.
///
/// Deliberadamente amplo: um falso positivo custa uma renomeação, e um falso negativo custa a
/// senha do usuário num arquivo de log.
const FORBIDDEN: &[&str] = &[
    "usage",
    "hid_usage",
    "keycode",
    "scancode",
    "key_event",
    "keys",
    "pressed_keys",
    "clipboard_text",
    "clip_text",
    "text_content",
    "typed",
    "password",
    "secret",
];

/// Procura logs que possam registrar conteúdo digitado.
pub(crate) fn check(files: &[RustFile]) -> Vec<Violation> {
    let mut violations = Vec::new();

    for file in files {
        if is_exempt(&file.path) {
            continue;
        }
        for (index, line) in file.text.lines().enumerate() {
            let Some(macro_name) = LOG_MACROS.iter().find(|name| line.contains(**name)) else {
                continue;
            };
            let Some(field) = FORBIDDEN.iter().find(|field| mentions(line, field)) else {
                continue;
            };
            violations.push(Violation {
                path: file.path.clone(),
                line: index + 1,
                message: format!(
                    "`{macro_name}` menciona `{field}`. O produto vê senhas; um log com \
                     conteúdo de tecla é a senha do usuário em arquivo. Registre o tipo e o \
                     tamanho, nunca o conteúdo (docs/04-seguranca.md §7)."
                ),
            });
        }
    }

    violations
}

/// Se um caminho está fora da regra.
///
/// Só este próprio arquivo, que precisa citar os nomes proibidos para procurá-los.
fn is_exempt(path: &str) -> bool {
    path.ends_with("xtask/src/logs.rs")
}

/// Se a linha menciona o nome como palavra, e não como pedaço de outra.
fn mentions(line: &str, needle: &str) -> bool {
    let bytes = line.as_bytes();
    let mut from = 0;
    while let Some(offset) = line.get(from..).and_then(|rest| rest.find(needle)) {
        let start = from + offset;
        let end = start + needle.len();
        let before_ok = start == 0 || !is_ident_byte(bytes.get(start - 1).copied());
        let after_ok = !is_ident_byte(bytes.get(end).copied());
        if before_ok && after_ok {
            return true;
        }
        from = end;
    }
    false
}

const fn is_ident_byte(byte: Option<u8>) -> bool {
    match byte {
        Some(b) => b.is_ascii_alphanumeric() || b == b'_',
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(text: &str) -> RustFile {
        RustFile {
            path: "crates/ir-daemon/src/x.rs".to_owned(),
            lines: text.lines().count(),
            production_lines: text.lines().count(),
            text: text.to_owned(),
        }
    }

    #[test]
    fn a_log_of_a_key_is_caught() {
        let found = check(&[file("    tracing::debug!(?usage, \"tecla\");")]);
        assert_eq!(found.len(), 1);
    }

    #[test]
    fn a_log_of_size_and_kind_is_allowed() {
        let found = check(&[file(
            "    tracing::info!(kind = \"texto\", bytes = 42, \"copiado\");",
        )]);
        assert!(found.is_empty(), "tipo e tamanho podem, conteúdo não");
    }

    #[test]
    fn a_line_without_a_log_macro_is_ignored() {
        let found = check(&[file("    let usage = HidUsage(0x04);")]);
        assert!(found.is_empty());
    }

    #[test]
    fn a_longer_identifier_that_merely_contains_the_word_is_not_a_false_positive() {
        let found = check(&[file("    tracing::debug!(usages_total = 3, \"resumo\");")]);
        assert!(found.is_empty(), "`usages_total` não é `usage`");
    }

    #[test]
    fn every_log_macro_is_covered() {
        for macro_name in LOG_MACROS {
            let line = format!("    tracing::{macro_name}(?password, \"x\");");
            assert_eq!(
                check(&[file(&line)]).len(),
                1,
                "{macro_name} não foi coberta"
            );
        }
    }

    #[test]
    fn this_file_is_exempt_so_it_can_name_what_it_forbids() {
        assert!(is_exempt("xtask/src/logs.rs"));
        assert!(!is_exempt("crates/ir-daemon/src/logs.rs"));
    }
}
