//! Em que pé está a cópia prometida — o que os fluxos de leitura consultam a cada bloco.
//!
//! Compartilhado entre quem promete (o ajudante, que sabe quando a cópia chegou ou parou) e os
//! fluxos que o Explorer está lendo, que podem sobreviver à promessa: a pessoa colou, a cópia foi
//! publicada, e o Explorer ainda está no meio do último arquivo.

use std::path::PathBuf;
use std::sync::{Mutex, PoisonError};

use crate::chegada::{Chegada, ItemDaChegada};

/// Onde a cópia está.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Situacao {
    /// Chegando: os itens estão na montagem.
    Chegando,
    /// Publicada: os itens estão onde a entrega foi publicada.
    Publicada,
    /// Não chega mais: quem lê recebe erro, e o Explorer o mostra.
    Falhou,
}

/// A cópia prometida e a situação dela.
#[derive(Debug)]
pub(super) struct Andamento {
    chegada: Chegada,
    situacao: Mutex<Situacao>,
}

impl Andamento {
    pub(super) fn novo(chegada: Chegada) -> Self {
        Self {
            chegada,
            situacao: Mutex::new(Situacao::Chegando),
        }
    }

    pub(super) fn itens(&self) -> &[ItemDaChegada] {
        &self.chegada.itens
    }

    pub(super) fn situacao(&self) -> Situacao {
        *self.situacao.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn mudar(&self, nova: Situacao) {
        let mut situacao = self.situacao.lock().unwrap_or_else(PoisonError::into_inner);
        // A falha é final: uma cópia que parou não volta a ser lida de onde foi publicada.
        if *situacao != Situacao::Falhou {
            *situacao = nova;
        }
    }

    /// A entrega foi publicada: dali em diante, os itens são lidos de onde ela ficou.
    pub(super) fn publicar(&self) {
        self.mudar(Situacao::Publicada);
    }

    /// A cópia não chega mais.
    pub(super) fn falhar(&self) {
        self.mudar(Situacao::Falhou);
    }

    /// Onde o item está agora. `None` para índice fora da lista.
    ///
    /// Antes da publicação, só a montagem: a entrega anterior de mesmo nome ainda pode estar no
    /// destino, e lê-la seria colar a cópia velha.
    pub(super) fn caminho(&self, indice: usize) -> Option<PathBuf> {
        let item = self.chegada.itens.get(indice)?;
        let base = match self.situacao() {
            Situacao::Publicada => &self.chegada.publicada_em,
            Situacao::Chegando | Situacao::Falhou => &self.chegada.montagem,
        };
        Some(item.em(base))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn andamento() -> Andamento {
        Andamento::novo(Chegada {
            montagem: PathBuf::from("montagem"),
            publicada_em: PathBuf::from("recebidos"),
            itens: vec![ItemDaChegada {
                caminho: "pasta/a.txt".to_owned(),
                tamanho: 3,
                pasta: false,
            }],
        })
    }

    #[test]
    fn o_item_e_lido_da_montagem_e_depois_de_onde_foi_publicado() {
        let andamento = andamento();
        assert_eq!(
            andamento.caminho(0),
            Some(PathBuf::from("montagem").join("pasta").join("a.txt"))
        );
        andamento.publicar();
        assert_eq!(
            andamento.caminho(0),
            Some(PathBuf::from("recebidos").join("pasta").join("a.txt"))
        );
        assert_eq!(andamento.caminho(1), None);
    }

    #[test]
    fn a_falha_e_final() {
        let andamento = andamento();
        andamento.falhar();
        andamento.publicar();
        assert_eq!(andamento.situacao(), Situacao::Falhou);
    }
}
