//! Quem é cada computador: identificador opaco e nome legível.

use serde::{Deserialize, Serialize};

use ir_proto::ids::MachineId;
use ir_proto::peer::MachineName;

/// Uma instalação, identificada de forma opaca.
///
/// A interface nunca interpreta estes bytes: ela os mostra agrupados para conferência e os devolve
/// ao serviço quando precisa nomear um par.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Maquina(pub [u8; 16]);

impl From<MachineId> for Maquina {
    fn from(maquina: MachineId) -> Self {
        Self(maquina.0)
    }
}

impl From<Maquina> for MachineId {
    fn from(maquina: Maquina) -> Self {
        Self(maquina.0)
    }
}

/// O nome legível de uma máquina.
///
/// Já vem consertado: sem caracteres de controle e dentro do limite. A validação acontece na
/// construção, e não na exibição, porque um valor que existe é um valor seguro de mostrar — um nome
/// com sequência de escape de terminal contamina log e interface.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(from = "String")]
pub struct Nome(String);

impl Nome {
    /// Constrói um nome de qualquer texto, consertando o que estiver fora das regras.
    #[must_use]
    pub fn coagido(texto: &str) -> Self {
        Self(MachineName::coagido(texto).as_str().to_owned())
    }

    /// O texto.
    #[must_use]
    pub fn como_texto(&self) -> &str {
        &self.0
    }
}

impl From<String> for Nome {
    fn from(texto: String) -> Self {
        Self::coagido(&texto)
    }
}

impl From<MachineName> for Nome {
    fn from(nome: MachineName) -> Self {
        Self(nome.as_str().to_owned())
    }
}

impl core::fmt::Display for Nome {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn um_nome_com_escape_de_terminal_e_limpo_na_construcao() {
        // Um nome vem do outro computador. Se ele carregar escape ANSI, contamina o log de quem o
        // recebe — é injeção pela porta dos fundos.
        let sujo = format!("banca{}[31mda", char::from(27));
        assert_eq!(Nome::coagido(&sujo).como_texto(), "banca[31mda");
    }

    #[test]
    fn um_nome_que_chega_vazio_nao_deixa_a_tela_sem_nada() {
        assert!(!Nome::coagido("   ").como_texto().is_empty());
    }
}
