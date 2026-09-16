//! As setas de dependência de `docs/02-arquitetura.md` §2.
//!
//! Foi a ausência dessas setas que permitiu ao InputRemote 1 acumular 10.491 linhas no crate
//! da interface — mais que transporte e plataforma somados. A interface tinha virado o
//! produto, e nada no repositório dizia que aquilo era proibido.
//!
//! Aqui elas são dado, e o CI as faz cumprir.

use std::path::Path;

use anyhow::{Context, Result};

use crate::scan::{Violation, manifests};

/// O que cada crate pode depender, dentro do workspace.
///
/// Uma lista vazia significa "de nada". `ir-proto` é o único assim, e é de propósito: ele é a
/// base sobre a qual todo o resto se apoia.
const ALLOWED: &[(&str, &[&str])] = &[
    ("ir-proto", &[]),
    ("ir-geometry", &["ir-proto"]),
    ("ir-session", &["ir-proto", "ir-geometry"]),
    ("ir-crypto", &["ir-proto"]),
    ("ir-ipc", &["ir-proto"]),
    ("ir-net", &["ir-proto", "ir-crypto"]),
    ("ir-bt", &["ir-proto", "ir-crypto"]),
    // A fronteira dos portadores: o unico lugar que conhece rede e radio ao mesmo tempo.
    (
        "ir-transporte",
        &["ir-proto", "ir-crypto", "ir-net", "ir-bt"],
    ),
    ("ir-files", &["ir-proto"]),
    ("ir-input", &["ir-proto"]),
    ("ir-clip", &["ir-proto"]),
    (
        "ir-daemon",
        &[
            "ir-proto",
            "ir-geometry",
            "ir-session",
            "ir-crypto",
            "ir-ipc",
            "ir-transporte",
            "ir-files",
            "ir-input",
        ],
    ),
    ("ir-agent", &["ir-proto", "ir-ipc", "ir-input", "ir-clip"]),
    // A interface não conhece o produto. É a fronteira que impede o v1 de acontecer de novo,
    // e também o que mantém a licença do Slint contida num binário só
    // (`docs/adr/0007-ui-slint-processo-separado.md`).
    ("ir-ui", &["ir-ipc"]),
];

/// Crates que **não podem** depender de nada que faça E/S.
///
/// A lista de proibidos não é de nomes de crate, é de capacidades: relógio, socket, arquivo,
/// runtime assíncrono. Se um deles entrar, o núcleo deixa de ser testável em microssegundos e
/// o argumento do ADR-0004 se desfaz.
const PURE: &[&str] = &["ir-proto", "ir-geometry", "ir-session"];

/// Dependências que denunciam E/S ou relógio.
const IMPURE: &[&str] = &[
    "tokio",
    "async-std",
    "smol",
    "mio",
    "socket2",
    "reqwest",
    "std-time",
    "chrono",
    "time",
    "rand",
    "getrandom",
    "windows",
    "rustix",
    "nix",
    "libc",
    "slint",
    "eframe",
    "egui",
];

/// Verifica as setas e a pureza.
pub(crate) fn check(root: &Path) -> Result<Vec<Violation>> {
    let mut violations = Vec::new();

    for (name, manifest) in manifests(root)? {
        let text = std::fs::read_to_string(&manifest)
            .with_context(|| format!("lendo {}", manifest.display()))?;
        let parsed: toml::Value = toml::from_str(&text)
            .with_context(|| format!("interpretando {}", manifest.display()))?;

        let deps = dependency_names(&parsed);
        let relative = manifest.strip_prefix(root).unwrap_or(&manifest);
        let path = relative.to_string_lossy().replace('\\', "/");

        violations.extend(check_arrows(&name, &deps, &path));
        violations.extend(check_purity(&name, &deps, &path));
    }

    Ok(violations)
}

fn dependency_names(manifest: &toml::Value) -> Vec<String> {
    let Some(table) = manifest.get("dependencies").and_then(toml::Value::as_table) else {
        return Vec::new();
    };
    table.keys().cloned().collect()
}

fn check_arrows(name: &str, deps: &[String], path: &str) -> Vec<Violation> {
    let Some((_, allowed)) = ALLOWED.iter().find(|(crate_name, _)| *crate_name == name) else {
        return vec![Violation {
            path: path.to_owned(),
            line: 0,
            message: format!(
                "crate `{name}` não está na tabela de setas de docs/02-arquitetura.md §2. \
                 Acrescente-o lá, com o que ele pode depender, antes de usá-lo."
            ),
        }];
    };

    deps.iter()
        .filter(|dep| dep.starts_with("ir-"))
        .filter(|dep| !allowed.contains(&dep.as_str()))
        .map(|dep| Violation {
            path: path.to_owned(),
            line: 0,
            message: format!(
                "`{name}` depende de `{dep}`, o que a tabela de docs/02-arquitetura.md §2 não \
                 permite. Se a dependência for mesmo necessária, ela precisa de um ADR — foi \
                 a ausência dessa fronteira que produziu o crate de 10.491 linhas do v1."
            ),
        })
        .collect()
}

fn check_purity(name: &str, deps: &[String], path: &str) -> Vec<Violation> {
    if !PURE.contains(&name) {
        return Vec::new();
    }

    deps.iter()
        .filter(|dep| IMPURE.contains(&dep.as_str()))
        .map(|dep| Violation {
            path: path.to_owned(),
            line: 0,
            message: format!(
                "`{name}` é um crate puro e depende de `{dep}`, que traz E/S, relógio ou API \
                 de sistema. Ver docs/09-padroes-de-codigo.md §3: o núcleo precisa rodar em \
                 microssegundos, sem periférico nenhum."
            ),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_pure_crate_is_in_the_arrow_table() {
        for name in PURE {
            assert!(
                ALLOWED.iter().any(|(crate_name, _)| crate_name == name),
                "`{name}` é declarado puro mas não está na tabela de setas"
            );
        }
    }

    #[test]
    fn the_interface_may_depend_on_nothing_but_ipc() {
        let (_, allowed) = ALLOWED
            .iter()
            .find(|(name, _)| *name == "ir-ui")
            .expect("ir-ui está na tabela");
        assert_eq!(
            *allowed,
            ["ir-ipc"],
            "a interface não pode conhecer o produto"
        );
    }

    #[test]
    fn the_protocol_crate_depends_on_nothing() {
        let (_, allowed) = ALLOWED
            .iter()
            .find(|(name, _)| *name == "ir-proto")
            .expect("ir-proto está na tabela");
        assert!(
            allowed.is_empty(),
            "ir-proto é a base; ele não se apoia em nada"
        );
    }

    #[test]
    fn an_unknown_crate_is_refused_instead_of_ignored() {
        let found = check_arrows("ir-misterioso", &[], "crates/x/Cargo.toml");
        assert_eq!(
            found.len(),
            1,
            "crate fora da tabela tem de ser recusado, não ignorado"
        );
    }

    #[test]
    fn a_forbidden_arrow_is_caught() {
        let found = check_arrows(
            "ir-ui",
            &["ir-session".to_owned()],
            "crates/ir-ui/Cargo.toml",
        );
        assert_eq!(found.len(), 1);
    }

    #[test]
    fn an_allowed_arrow_passes() {
        let found = check_arrows(
            "ir-session",
            &["ir-proto".to_owned()],
            "crates/ir-session/Cargo.toml",
        );
        assert!(found.is_empty());
    }

    #[test]
    fn a_runtime_in_a_pure_crate_is_caught() {
        let found = check_purity("ir-session", &["tokio".to_owned()], "x");
        assert_eq!(
            found.len(),
            1,
            "tokio no núcleo desfaz o argumento do ADR-0004"
        );
    }

    #[test]
    fn a_runtime_outside_a_pure_crate_is_fine() {
        assert!(check_purity("ir-daemon", &["tokio".to_owned()], "x").is_empty());
    }
}
