//! Os arquivos que começaram a chegar, contados ao ajudante de clipboard antes do fim.
//!
//! É o que permite colar antes de a cópia chegar: com a lista de itens e o lugar onde eles estão
//! sendo gravados, o ajudante põe no clipboard arquivos **virtuais** — o Explorer cola na hora, e
//! lê cada um à medida que ele chega ([log 56](../../../docs/logs/56-o-ajudante-que-sobreviveu.md)).
//!
//! Só vai ao ajudante de clipboard ([`crate::Aviso::so_para_o_ajudante`]): os caminhos são da pasta
//! de recebidos desta máquina, e a janela não tem o que fazer com eles.

use serde::{Deserialize, Serialize};

/// Uma cópia que começou a chegar.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Chegando {
    /// O nome da entrega, o mesmo das [`crate::Transferencia`] desta cópia.
    pub nome: String,
    /// Onde os itens estão sendo gravados enquanto chegam: a montagem, que só vira entrega no fim.
    pub montagem: String,
    /// Onde os mesmos caminhos relativos ficam depois de publicada a entrega.
    pub publicada_em: String,
    /// Os itens, na ordem do manifesto: pastas antes do que há dentro delas.
    pub itens: Vec<ItemChegando>,
}

/// Um item da cópia que está chegando.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ItemChegando {
    /// Caminho relativo à montagem, com `/`, a partir da raiz que o usuário copiou.
    pub caminho: String,
    /// Tamanho em bytes; zero para pasta.
    pub tamanho: u64,
    /// Se é pasta.
    pub pasta: bool,
}
