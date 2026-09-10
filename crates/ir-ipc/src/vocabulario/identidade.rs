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

impl Maquina {
    /// A impressão digital, em grupos de quatro dígitos hexadecimais.
    ///
    /// Agrupada porque a comparação é feita por uma pessoa olhando duas telas, e 32 caracteres
    /// corridos não se comparam sem erro.
    #[must_use]
    pub fn impressao(&self) -> String {
        let mut texto = String::with_capacity(39);
        for (posicao, byte) in self.0.iter().enumerate() {
            if posicao != 0 && posicao % 2 == 0 {
                texto.push(' ');
            }
            texto.push(hexadecimal(byte >> 4));
            texto.push(hexadecimal(byte & 0x0F));
        }
        texto
    }
}

/// Um dígito hexadecimal minúsculo.
///
/// O ramo impossível devolve `?` em vez de entrar em pânico. A entrada é meio byte e nunca passa de
/// 15, mas um caractere estranho numa impressão digital é um defeito visível e corrigível — uma
/// janela que fecha sozinha não é.
fn hexadecimal(meio: u8) -> char {
    match meio {
        0..=9 => char::from(b'0' + meio),
        10..=15 => char::from(b'a' + meio - 10),
        _ => '?',
    }
}

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
    fn a_impressao_vem_agrupada_para_dar_para_comparar() {
        let texto = Maquina([0xAB; 16]).impressao();
        assert_eq!(texto.split(' ').count(), 8, "{texto}");
        for grupo in texto.split(' ') {
            assert_eq!(grupo.len(), 4, "{texto}");
        }
        assert!(
            !texto.contains("ABAB"),
            "hexadecimal em minúsculas: {texto}"
        );
    }

    #[test]
    fn maquinas_diferentes_tem_impressoes_diferentes() {
        assert_ne!(Maquina([1; 16]).impressao(), Maquina([2; 16]).impressao());
    }

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
