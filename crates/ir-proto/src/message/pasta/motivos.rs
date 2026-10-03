//! Os motivos de uma recusa na pasta compartilhada: da pasta oferecida, do trecho pedido e da
//! operação da réplica. Cada enum é formato de fio: variante nova entra **no fim**.

use serde::{Deserialize, Serialize};

/// Por que a réplica recusou uma pasta oferecida.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum DeclineFolder {
    /// O usuário disse não.
    UserDeclined,
    /// Não cabe no disco da réplica.
    NoDiskSpace,
    /// A réplica já tem pastas demais.
    TooManyFolders,
}

/// Por que um trecho não pôde ser servido.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum RangeFailure {
    /// A entrada mudou de versão: a réplica precisa das mudanças antes de pedir de novo.
    Stale,
    /// A entrada não existe mais.
    Missing,
    /// O arquivo existe, mas não pôde ser lido agora — aberto com exclusividade por outro programa.
    Unreadable,
}

/// Por que uma operação da réplica não foi aplicada.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum Refusal {
    /// Caminho que não pode ser gravado com segurança.
    UnsafePath,
    /// A origem não tem espaço.
    NoDiskSpace,
    /// A pasta não é compartilhada com quem pediu.
    UnknownFolder,
    /// O arquivo está aberto com exclusividade por outro programa; vale tentar de novo depois.
    Locked,
    /// O resumo dos bytes recebidos não confere com o anunciado.
    HashMismatch,
    /// A pasta chegou ao máximo de entradas.
    TooManyEntries,
}
