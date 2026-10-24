//! O painel no Android, visto pela janela: parear, conferir o código, ver quem está ligado, esquecer
//! ([12](../../../docs/12-painel-android.md)).
//!
//! Vocabulário próprio, como o das pastas: a janela não conhece o enlace do painel nem as chaves,
//! só o que mostra e o que pede.

use serde::{Deserialize, Serialize};

/// Um painel pareado, como a janela o mostra.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResumoDoPainel {
    /// Como pedir para esquecê-lo ([`ComandoDePainel::Esquecer`]). Opaco para a janela.
    pub id: String,
    /// O nome que o aparelho disse.
    pub nome: String,
    /// Se está conectado agora.
    pub conectado: bool,
}

/// O que a janela pede sobre os painéis.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ComandoDePainel {
    /// Abra a janela de pareamento: o painel novo pode chegar nos próximos três minutos.
    Parear,
    /// O código apareceu igual no aparelho?
    Confirmar(bool),
    /// Desista do pareamento em curso, e feche a janela.
    Cancelar,
    /// Esqueça este painel: ele é desconectado na hora e não volta sem parear de novo.
    Esquecer(String),
}

/// O que o serviço conta à janela sobre os painéis.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum AvisoDoPainel {
    /// Confira este código no aparelho. Um dígito por posição, para as caixas da tela.
    Codigo([u8; 6]),
    /// O pareamento terminou: `true` se deu certo.
    Concluido(bool),
    /// A lista mudou — um painel novo, esquecido, que ligou, desligou ou disse o nome.
    Lista(Vec<ResumoDoPainel>),
}

/// Uma linha sobre os aparelhos, para o cartão da tela inicial.
#[must_use]
pub fn resumo_dos_paineis(paineis: &[ResumoDoPainel]) -> String {
    let ligados = paineis.iter().filter(|p| p.conectado).count();
    let primeiro = paineis.first().map_or("", |p| p.nome.as_str());
    match (paineis.len(), ligados) {
        (0, _) => "Veja o uso deste computador num tablet ou celular.".to_owned(),
        (1, 1) => format!("{primeiro} está mostrando este computador agora."),
        (1, _) => format!("{primeiro} pareado, desconectado agora."),
        (total, 0) => format!("{total} aparelhos pareados, nenhum conectado agora."),
        (total, ligados) => format!("{total} aparelhos pareados, {ligados} conectado(s) agora."),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn painel(nome: &str, conectado: bool) -> ResumoDoPainel {
        ResumoDoPainel {
            id: nome.to_owned(),
            nome: nome.to_owned(),
            conectado,
        }
    }

    #[test]
    fn o_resumo_diz_quem_esta_olhando() {
        assert!(resumo_dos_paineis(&[]).contains("tablet ou celular"));
        assert_eq!(
            resumo_dos_paineis(&[painel("Tablet", true)]),
            "Tablet está mostrando este computador agora."
        );
        assert_eq!(
            resumo_dos_paineis(&[painel("Tablet", false)]),
            "Tablet pareado, desconectado agora."
        );
        assert_eq!(
            resumo_dos_paineis(&[painel("a", false), painel("b", true)]),
            "2 aparelhos pareados, 1 conectado(s) agora."
        );
        assert_eq!(
            resumo_dos_paineis(&[painel("a", false), painel("b", false)]),
            "2 aparelhos pareados, nenhum conectado agora."
        );
    }
}
