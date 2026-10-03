//! Os números de inode do sistema de arquivos e os caminhos relativos a que eles correspondem.
//!
//! O núcleo fala em inode; o cache, em caminho. O número de um caminho fica o mesmo enquanto o
//! processo viver, e um `rename` muda o caminho do número — e dos que estão abaixo dele.

use std::collections::HashMap;

/// O caminho da raiz, vazio.
pub(super) const RAIZ: &str = "";

#[derive(Debug)]
pub(super) struct Inodes {
    por_numero: HashMap<u64, String>,
    por_caminho: HashMap<String, u64>,
    proximo: u64,
}

impl Inodes {
    pub(super) fn novos() -> Self {
        let mut inodes = Self {
            por_numero: HashMap::new(),
            por_caminho: HashMap::new(),
            proximo: 2,
        };
        inodes.por_numero.insert(1, RAIZ.to_owned());
        inodes.por_caminho.insert(RAIZ.to_owned(), 1);
        inodes
    }

    /// O caminho deste número.
    pub(super) fn caminho(&self, numero: u64) -> Option<String> {
        self.por_numero.get(&numero).cloned()
    }

    /// O número deste caminho, dando um novo se ainda não tem.
    pub(super) fn numero(&mut self, caminho: &str) -> u64 {
        if let Some(numero) = self.por_caminho.get(caminho) {
            return *numero;
        }
        let numero = self.proximo;
        self.proximo += 1;
        self.por_numero.insert(numero, caminho.to_owned());
        self.por_caminho.insert(caminho.to_owned(), numero);
        numero
    }

    /// O caminho saiu do disco.
    pub(super) fn esquecer(&mut self, caminho: &str) {
        if let Some(numero) = self.por_caminho.remove(caminho) {
            self.por_numero.remove(&numero);
        }
    }

    /// `de` virou `para`, e tudo abaixo dele junto.
    pub(super) fn renomear(&mut self, de: &str, para: &str) {
        self.esquecer(para);
        let prefixo = format!("{de}/");
        let mudam: Vec<(String, u64)> = self
            .por_caminho
            .iter()
            .filter(|(c, _)| c.as_str() == de || c.starts_with(&prefixo))
            .map(|(c, n)| (c.clone(), *n))
            .collect();
        for (antigo, numero) in mudam {
            let novo = format!("{para}{}", antigo.get(de.len()..).unwrap_or_default());
            self.por_caminho.remove(&antigo);
            self.por_caminho.insert(novo.clone(), numero);
            self.por_numero.insert(numero, novo);
        }
    }
}

/// `pai/nome`, ou só `nome` na raiz.
pub(super) fn juntar(pai: &str, nome: &str) -> String {
    if pai.is_empty() {
        nome.to_owned()
    } else {
        format!("{pai}/{nome}")
    }
}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn renomear_leva_o_que_esta_abaixo() {
        let mut inodes = Inodes::novos();
        let pasta = inodes.numero("a");
        let dentro = inodes.numero("a/b.txt");
        let vizinho = inodes.numero("ab");
        inodes.renomear("a", "c");
        assert_eq!(inodes.caminho(pasta).as_deref(), Some("c"));
        assert_eq!(inodes.caminho(dentro).as_deref(), Some("c/b.txt"));
        assert_eq!(
            inodes.caminho(vizinho).as_deref(),
            Some("ab"),
            "prefixo de nome não é pasta"
        );
        assert_eq!(inodes.numero("c/b.txt"), dentro);
    }
}
