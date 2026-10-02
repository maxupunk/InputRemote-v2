//! Arquivos que estão chegando, do jeito que o clipboard os promete.
//!
//! Forma canônica, como [`crate::Conteudo`]: quem traduz o aviso do serviço para isto é o ajudante,
//! e o `ir-clip` não conhece o canal local ([02, §2](../../../docs/02-arquitetura.md)).

use std::path::{Path, PathBuf};

/// Uma cópia que está chegando: onde cada item está agora, onde vai ficar, e quais são.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Chegada {
    /// Onde os itens estão sendo gravados enquanto chegam.
    pub montagem: PathBuf,
    /// Onde os mesmos caminhos relativos ficam depois de publicada a entrega.
    pub publicada_em: PathBuf,
    /// Os itens, pastas antes do que há dentro delas.
    pub itens: Vec<ItemDaChegada>,
}

/// Um item da cópia que está chegando.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ItemDaChegada {
    /// Caminho relativo, com `/`, a partir da raiz que o usuário copiou.
    pub caminho: String,
    /// Tamanho em bytes; zero para pasta.
    pub tamanho: u64,
    /// Se é pasta.
    pub pasta: bool,
}

impl ItemDaChegada {
    /// O caminho deste item dentro de `base` — a montagem, ou onde a entrega foi publicada.
    ///
    /// Componente a componente: um caminho relativo vindo de outra máquina não é colado como texto
    /// num caminho local. O serviço já recusou o que escaparia da pasta; aqui só se monta.
    #[must_use]
    pub fn em(&self, base: &Path) -> PathBuf {
        let mut caminho = base.to_path_buf();
        for parte in self.caminho.split('/').filter(|parte| !parte.is_empty()) {
            caminho.push(parte);
        }
        caminho
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn o_item_e_achado_componente_a_componente() {
        let item = ItemDaChegada {
            caminho: "relatório/anexos/b.txt".to_owned(),
            tamanho: 7,
            pasta: false,
        };
        let base = Path::new("recebidos");
        assert_eq!(
            item.em(base),
            base.join("relatório").join("anexos").join("b.txt")
        );
    }
}
