//! Texto em UTF-8 lido como Windows-1252 e gravado de novo: "está" vira "estÃ¡".
//!
//! Aconteceu com um arquivo inteiro do `ir-transporte`, que foi para o repositório assim e compilou
//! — comentário não quebra a build, e a frase de uma tela quebraria só na tela. A marca é
//! inconfundível: `Ã` ou `Â` seguidos de um caractere que era um byte de continuação, ou o `â€` que
//! sobra de travessões e aspas curvas. Nenhuma palavra em português tem isso.

use crate::scan::{RustFile, Violation};

/// Procura texto duplamente codificado.
pub(crate) fn check(files: &[RustFile]) -> Vec<Violation> {
    let mut violations = Vec::new();
    for file in files {
        if file.path.ends_with("xtask/src/texto.rs") {
            continue;
        }
        for (index, line) in file.text.lines().enumerate() {
            if !duplamente_codificado(line) {
                continue;
            }
            violations.push(Violation {
                path: file.path.clone(),
                line: index + 1,
                message: "texto em UTF-8 gravado de novo como se fosse Windows-1252 (\"estÃ¡\" em \
                          vez de \"está\"). Desfaça no arquivo inteiro: leia como UTF-8, codifique \
                          em Windows-1252 e decodifique como UTF-8."
                    .to_owned(),
            });
        }
    }
    violations
}

/// Se a linha tem a marca da dupla codificação.
fn duplamente_codificado(linha: &str) -> bool {
    if linha.contains("â€") {
        return true;
    }
    let mut anterior = ' ';
    for atual in linha.chars() {
        if matches!(anterior, 'Ã' | 'Â') && era_byte_de_continuacao(atual) {
            return true;
        }
        anterior = atual;
    }
    false
}

/// Se o caractere é o que o Windows-1252 mostra para um byte de 0x80 a 0xBF.
fn era_byte_de_continuacao(c: char) -> bool {
    ('\u{a0}'..='\u{bf}').contains(&c) || "€‚ƒ„…†‡ˆ‰Š‹ŒŽ‘’“”•–—˜™š›œžŸ".contains(c)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pega_o_portugues_estragado() {
        for linha in [
            "estÃ¡",
            "configuraÃ§Ã£o",
            "Ã\u{a0}pergunta",
            "a â€” b",
            "nÂº",
        ] {
            assert!(duplamente_codificado(linha), "{linha}");
        }
    }

    #[test]
    fn deixa_o_portugues_certo_e_letras_soltas() {
        for linha in [
            "está",
            "configuração",
            "à pergunta",
            "a — b",
            "Ãgua",
            "SÃO PAULO",
        ] {
            assert!(!duplamente_codificado(linha), "{linha}");
        }
    }
}
