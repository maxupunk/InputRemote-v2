//! O tipo de erro do crate.

/// Resultado das operações de entrada.
pub type Result<T> = core::result::Result<T, InputError>;

/// O que pode dar errado ao capturar ou injetar.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum InputError {
    /// Não há backend para esta plataforma ou papel.
    #[error("captura ou injeção não suportada nesta plataforma")]
    Unsupported,

    /// Falha ao abrir ou criar o dispositivo de entrada.
    #[error("não foi possível abrir o dispositivo de entrada: {0}")]
    Device(String),

    /// O sistema recusou a injeção.
    ///
    /// No Windows, é o sintoma do endurecimento de janeiro de 2026 quando as origens confiáveis
    /// não são atendidas ([05, §4.4](../../../docs/05-windows.md)).
    #[error("o sistema recusou a injeção de entrada")]
    Rejected,

    /// A tecla recebida não tem correspondência no mapa desta plataforma.
    ///
    /// Não é fatal: teclas exóticas são ignoradas, não derrubam a sessão.
    #[error("tecla sem correspondência no mapa da plataforma")]
    UnmappedKey,

    /// Falha de E/S ao falar com o dispositivo.
    #[error("erro de E/S no dispositivo de entrada: {0}")]
    Io(String),
}
