//! O que o disco mostrou numa varredura: o retrato da pasta.
//!
//! Quem varre é o `ir-sincronia`; aqui chega só o resultado, já sem o que [`crate::ignorar`] tira.
//! O motor compara o retrato com o índice e decide o que mudou — sem tocar o disco.

use std::collections::BTreeMap;

use ir_proto::message::EntryKind;
use serde::{Deserialize, Serialize};

/// Uma entrada como o disco a mostrou.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Visto {
    /// Arquivo ou subpasta.
    pub tipo: EntryKind,
    /// Tamanho em bytes; zero para subpasta.
    pub tamanho: u64,
    /// Última modificação, em nanossegundos desde 1970.
    pub modificado_ns: i64,
    /// O BLAKE3 do conteúdo, quando já foi calculado.
    pub resumo: Option<[u8; 32]>,
}

impl Visto {
    /// Uma subpasta.
    #[must_use]
    pub const fn pasta(modificado_ns: i64) -> Self {
        Self {
            tipo: EntryKind::Directory,
            tamanho: 0,
            modificado_ns,
            resumo: None,
        }
    }

    /// Um arquivo.
    #[must_use]
    pub const fn arquivo(tamanho: u64, modificado_ns: i64, resumo: Option<[u8; 32]>) -> Self {
        Self {
            tipo: EntryKind::File,
            tamanho,
            modificado_ns,
            resumo,
        }
    }

    /// Se o disco mostra o mesmo que antes, pelo que se vê sem ler o conteúdo.
    ///
    /// Tamanho e horário de modificação, como fazem Syncthing e rsync: ler cada arquivo a cada
    /// varredura custaria o disco inteiro. Uma subpasta não muda por dentro do que se vê aqui — o
    /// horário dela muda a cada arquivo criado lá dentro, e isso não é mudança da subpasta.
    #[must_use]
    pub fn igual_por_fora(&self, outro: &Self) -> bool {
        if self.tipo != outro.tipo {
            return false;
        }
        self.tipo == EntryKind::Directory
            || (self.tamanho == outro.tamanho && self.modificado_ns == outro.modificado_ns)
    }

    /// Se é subpasta.
    #[must_use]
    pub fn eh_pasta(&self) -> bool {
        self.tipo == EntryKind::Directory
    }
}

/// O retrato inteiro: caminho relativo, com `/`, e o que se viu nele.
pub type Retrato = BTreeMap<String, Visto>;

/// Os caminhos de que `caminho` depende: as pastas acima dele, da mais alta para a mais baixa.
pub(crate) fn ancestrais(caminho: &str) -> impl Iterator<Item = &str> {
    caminho
        .match_indices('/')
        .map(move |(posicao, _)| caminho.get(..posicao).unwrap_or(""))
}

/// Se `caminho` está dentro de `pasta` (e não é ela).
pub(crate) fn dentro_de(caminho: &str, pasta: &str) -> bool {
    caminho.len() > pasta.len()
        && caminho.starts_with(pasta)
        && caminho.as_bytes().get(pasta.len()) == Some(&b'/')
}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn os_ancestrais_vem_de_cima_para_baixo() {
        let todos: Vec<&str> = ancestrais("a/b/c.txt").collect();
        assert_eq!(todos, vec!["a", "a/b"]);
        assert_eq!(ancestrais("c.txt").count(), 0);
    }

    #[test]
    fn dentro_e_so_o_que_esta_abaixo() {
        assert!(dentro_de("a/b", "a"));
        assert!(!dentro_de("a", "a"));
        assert!(!dentro_de("ab/c", "a"), "prefixo de nome não é pasta");
    }

    #[test]
    fn subpasta_nao_muda_pelo_horario() {
        assert!(Visto::pasta(1).igual_por_fora(&Visto::pasta(2)));
        assert!(!Visto::arquivo(1, 1, None).igual_por_fora(&Visto::arquivo(1, 2, None)));
        assert!(!Visto::pasta(1).igual_por_fora(&Visto::arquivo(0, 1, None)));
    }
}
