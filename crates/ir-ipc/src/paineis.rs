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
