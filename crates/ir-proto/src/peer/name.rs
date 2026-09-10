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

    /// O nome usado quando não sobra nada de aproveitável.
    pub const SEM_NOME: &'static str = "computador";

    /// Constrói um nome de um texto qualquer, consertando o que estiver fora das regras.
    ///
    /// Existe para uma situação concreta: o nome da máquina costuma vir do sistema — hostname,
    /// nome de computador — e nada garante que ele caiba em [`limits::MAX_MACHINE_NAME`] ou que
    /// não tenha caractere de controle. Um serviço que se recusa a subir porque o hostname é
    /// comprido seria pior que um serviço com o nome cortado.
    ///
    /// Corta em limite de caractere, nunca no meio de um, para não produzir UTF-8 inválido.
    #[must_use]
    pub fn coagido(texto: &str) -> Self {
        let limpo: String = texto.chars().filter(|c| !c.is_control()).collect();

        let mut cortado = String::with_capacity(limits::MAX_MACHINE_NAME);
        for caractere in limpo.trim().chars() {
            if cortado.len() + caractere.len_utf8() > limits::MAX_MACHINE_NAME {
                break;
            }
            cortado.push(caractere);
        }

        let cortado = cortado.trim();
        if cortado.is_empty() {
            // Um nome vazio na tela é pior que um nome genérico: o usuário fica sem saber se
            // está vendo a máquina certa.
            return Self(Self::SEM_NOME.to_owned());
        }
        Self(cortado.to_owned())
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
mod coagido_tests {
    use super::*;

    #[test]
    fn um_nome_valido_atravessa_intacto() {
        assert_eq!(MachineName::coagido("bancada").as_str(), "bancada");
    }

    #[test]
    fn um_hostname_comprido_e_cortado_em_vez_de_recusado() {
        let comprido = "a".repeat(limits::MAX_MACHINE_NAME + 40);
        let nome = MachineName::coagido(&comprido);
        assert_eq!(nome.as_str().len(), limits::MAX_MACHINE_NAME);
        // E o resultado precisa continuar válido pelas regras normais.
        assert!(MachineName::new(nome.as_str()).is_ok());
    }

    #[test]
    fn o_corte_respeita_limite_de_caractere() {
        // Cada "é" ocupa 2 bytes; cortar no meio produziria UTF-8 inválido, que é o tipo de
        // defeito que só aparece na máquina de outra pessoa.
        let acentuado = "é".repeat(limits::MAX_MACHINE_NAME);
        let nome = MachineName::coagido(&acentuado);
        assert!(nome.as_str().len() <= limits::MAX_MACHINE_NAME);
        assert!(nome.as_str().chars().all(|c| c == 'é'));
        assert!(MachineName::new(nome.as_str()).is_ok());
    }

    #[test]
    fn sequencia_de_escape_de_terminal_nao_sobrevive() {
        // Um nome com escape ANSI contamina log e terminal. Coagir precisa limpar, e não só
        // cortar tamanho.
        let sujo = format!("banca{}[31mda", char::from(27));
        assert_eq!(MachineName::coagido(&sujo).as_str(), "banca[31mda");
    }

    #[test]
    fn o_que_nao_sobra_nada_vira_nome_generico() {
        let brancos: String = [char::from(9), char::from(10), char::from(0)]
            .iter()
            .collect();
        for entrada in ["", "   ", brancos.as_str()] {
            assert_eq!(
                MachineName::coagido(entrada).as_str(),
                MachineName::SEM_NOME
            );
        }
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
