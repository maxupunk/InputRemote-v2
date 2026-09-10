//! O nome legível de uma máquina.
//!
//! Vem do fio e vai para a interface e para o log. Por isso é validado na construção, e não
//! na exibição: um valor que existe é um valor já seguro de mostrar.

use serde::{Deserialize, Serialize};

use crate::error::{ProtoError, Result};
use crate::limits;

/// Nome legível de uma máquina, com tamanho limitado.
///
/// Vem do fio e é exibido na interface, então é validado na construção: tamanho limitado,
/// sem caracteres de controle. Um nome com `\n` ou com sequência de escape de terminal
/// contamina log e interface — é injeção pela porta dos fundos.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct MachineName(String);

impl MachineName {
    /// Valida e constrói.
    ///
    /// # Errors
    ///
    /// - [`ProtoError::TooLarge`] acima de [`limits::MAX_MACHINE_NAME`] bytes.
    /// - [`ProtoError::Malformed`] se houver caractere de controle ou se ficar vazio depois
    ///   de aparar espaços.
    pub fn new(name: impl Into<String>) -> Result<Self> {
        let name: String = name.into();
        let trimmed = name.trim();
        if trimmed.is_empty() {
            return Err(ProtoError::Malformed);
        }
        if trimmed.len() > limits::MAX_MACHINE_NAME {
            return Err(ProtoError::TooLarge {
                actual: trimmed.len(),
                limit: limits::MAX_MACHINE_NAME,
            });
        }
        if trimmed.chars().any(char::is_control) {
            return Err(ProtoError::Malformed);
        }
        Ok(Self(trimmed.to_owned()))
    }

    /// O texto validado.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for MachineName {
    type Error = ProtoError;

    fn try_from(name: String) -> Result<Self> {
        Self::new(name)
    }
}

impl From<MachineName> for String {
    fn from(name: MachineName) -> Self {
        name.0
    }
}

impl core::fmt::Display for MachineName {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn machine_name_trims_and_accepts_ordinary_names() {
        let name = MachineName::new("  Notebook do Maxuel  ").unwrap();
        assert_eq!(name.as_str(), "Notebook do Maxuel");
    }

    #[test]
    fn machine_name_refuses_control_characters() {
        for bad in ["linha\numa", "tab\tno meio", "escape\u{1b}[31m"] {
            assert_eq!(
                MachineName::new(bad).unwrap_err(),
                ProtoError::Malformed,
                "{bad:?}"
            );
        }
    }

    #[test]
    fn machine_name_refuses_empty_and_whitespace_only() {
        for bad in ["", "   ", "\t"] {
            assert!(MachineName::new(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn machine_name_refuses_oversized_input() {
        let long = "a".repeat(limits::MAX_MACHINE_NAME + 1);
        let err = MachineName::new(long).unwrap_err();
        assert!(
            matches!(err, ProtoError::TooLarge { limit, .. } if limit == limits::MAX_MACHINE_NAME)
        );
    }

    #[test]
    fn machine_name_accepts_exactly_the_limit() {
        let exact = "a".repeat(limits::MAX_MACHINE_NAME);
        assert!(MachineName::new(exact).is_ok());
    }
}
