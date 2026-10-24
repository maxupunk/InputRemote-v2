//! Os processos de uma máquina, como o painel os vê: agrupados pelo nome, com processador e memória.
//!
//! Agrupados porque é assim que a pessoa pensa — "o Chrome está pesado", e não "o processo 4812" —,
//! e porque o grupo cabe no recado entre os computadores, onde a lista de processos soltos não
//! caberia ([`crate::entre_pcs`]).

use serde::{Deserialize, Serialize};

/// Quantos processos vigiados (os cards de processo) cabem num pedido.
pub const MAX_VIGIADOS: usize = 4;

/// Quantos grupos vão na lista: os mais pesados em processador somados aos mais pesados em memória.
pub const MAX_LISTA: usize = 10;

/// O maior nome de grupo, em bytes. Nome de executável é curto; o teto mantém o recado no rádio.
pub const NOME_MAXIMO: usize = 20;

/// Um grupo de processos com o mesmo nome — ou, num processo vigiado, os que casam com o nome pedido.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UsoDeProcesso {
    /// O nome do executável, sem extensão; num vigiado, o nome que a pessoa pediu.
    pub nome: String,
    /// Quantos processos há no grupo. Zero num vigiado é "não está rodando".
    pub instancias: u16,
    /// O processador que o grupo usa, em milésimos da máquina inteira.
    pub cpu_permille: u16,
    /// A memória em uso, em KiB.
    pub memoria_kib: u64,
}

/// Os processos de uma máquina neste segundo.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Processos {
    /// Um por nome vigiado, na ordem do pedido.
    pub vigiados: Vec<UsoDeProcesso>,
    /// Os mais pesados, quando a lista foi pedida; vazia quando não foi.
    pub lista: Vec<UsoDeProcesso>,
}

impl Processos {
    /// Dentro dos tetos: nomes cortados sem partir caractere, listas do tamanho máximo.
    #[must_use]
    pub fn nos_tetos(mut self) -> Self {
        self.vigiados.truncate(MAX_VIGIADOS);
        self.lista.truncate(MAX_LISTA);
        for uso in self.vigiados.iter_mut().chain(self.lista.iter_mut()) {
            uso.nome = cortar(&uso.nome, NOME_MAXIMO).to_owned();
        }
        self
    }

    /// Só o que este interesse pede: os vigiados com os nomes dele, e a lista se ele a quer. A
    /// medida é uma só para o painel daqui e o do par; cada um recebe a sua parte. Interesse
    /// vazio, nada.
    #[must_use]
    pub fn para(&self, interesse: &InteresseEmProcessos) -> Option<Self> {
        if interesse.vazio() {
            return None;
        }
        let pedido = |uso: &&UsoDeProcesso| {
            interesse
                .nomes
                .iter()
                .any(|nome| nome.eq_ignore_ascii_case(&uso.nome))
        };
        Some(Self {
            vigiados: self.vigiados.iter().filter(pedido).cloned().collect(),
            lista: if interesse.lista {
                self.lista.clone()
            } else {
                Vec::new()
            },
        })
    }
}

/// O que se quer saber dos processos de uma máquina.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct InteresseEmProcessos {
    /// Os nomes vigiados: um processo casa com o nome se o contém, sem caixa.
    pub nomes: Vec<String>,
    /// Se a lista dos mais pesados foi pedida.
    pub lista: bool,
}

impl InteresseEmProcessos {
    /// Se não há nada a medir: aí os processos não são lidos.
    #[must_use]
    pub fn vazio(&self) -> bool {
        self.nomes.is_empty() && !self.lista
    }

    /// Os dois interesses juntos: os nomes de um e de outro sem repetir, e a lista se um a pediu.
    #[must_use]
    pub fn juntar(&self, outro: &Self) -> Self {
        let mut nomes = self.nomes.clone();
        for nome in &outro.nomes {
            if !nomes.iter().any(|n| n.eq_ignore_ascii_case(nome)) {
                nomes.push(nome.clone());
            }
        }
        Self {
            nomes,
            lista: self.lista || outro.lista,
        }
        .nos_tetos()
    }

    /// Dentro dos tetos: nomes aparados, não vazios, cortados, no máximo [`MAX_VIGIADOS`].
    #[must_use]
    pub fn nos_tetos(mut self) -> Self {
        self.nomes = self
            .nomes
            .iter()
            .map(|n| cortar(n.trim(), NOME_MAXIMO).to_owned())
            .filter(|n| !n.is_empty())
            .take(MAX_VIGIADOS)
            .collect();
        self
    }
}

/// Se o nome de um processo casa com o nome vigiado: contém, sem caixa. `wsl` casa com `wslhost`
/// e com `VmmemWSL`, onde mora a memória do WSL; `docker` casa com `com.docker.backend`.
#[must_use]
pub fn casa(processo: &str, vigiado: &str) -> bool {
    !vigiado.is_empty() && processo.to_lowercase().contains(&vigiado.to_lowercase())
}

/// O maior prefixo de `texto` com até `maximo` bytes, sem partir um caractere.
pub(crate) fn cortar(texto: &str, maximo: usize) -> &str {
    let mut fim = texto.len().min(maximo);
    while !texto.is_char_boundary(fim) {
        fim -= 1;
    }
    texto.get(..fim).unwrap_or("")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn uso(nome: &str) -> UsoDeProcesso {
        UsoDeProcesso {
            nome: nome.to_owned(),
            instancias: 1,
            cpu_permille: 1,
            memoria_kib: 1,
        }
    }

    #[test]
    fn casar_e_conter_sem_caixa() {
        assert!(casa("VmmemWSL", "wsl"));
        assert!(casa("com.docker.backend", "Docker"));
        assert!(!casa("chrome", "docker"));
        assert!(!casa("chrome", ""));
    }

    #[test]
    fn juntar_interesses_sem_repetir() {
        let a = InteresseEmProcessos {
            nomes: vec!["docker".to_owned()],
            lista: false,
        };
        let b = InteresseEmProcessos {
            nomes: vec!["Docker".to_owned(), "wsl".to_owned()],
            lista: true,
        };
        let junto = a.juntar(&b);
        assert_eq!(junto.nomes, vec!["docker", "wsl"]);
        assert!(junto.lista);
        assert!(InteresseEmProcessos::default().vazio());
    }

    #[test]
    fn cada_interesse_recebe_so_a_sua_parte() {
        let medido = Processos {
            vigiados: vec![uso("docker"), uso("wsl")],
            lista: vec![uso("chrome")],
        };
        let so_wsl = InteresseEmProcessos {
            nomes: vec!["WSL".to_owned()],
            lista: false,
        };
        let parte = medido.para(&so_wsl).unwrap();
        assert_eq!(parte.vigiados, vec![uso("wsl")]);
        assert!(parte.lista.is_empty());
        assert_eq!(medido.para(&InteresseEmProcessos::default()), None);
    }

    #[test]
    fn os_tetos_cortam_nomes_e_listas() {
        let longo = "ç".repeat(NOME_MAXIMO);
        let processos = Processos {
            vigiados: vec![uso(&longo); MAX_VIGIADOS + 2],
            lista: vec![uso("x"); MAX_LISTA + 5],
        }
        .nos_tetos();
        assert_eq!(processos.vigiados.len(), MAX_VIGIADOS);
        assert_eq!(processos.lista.len(), MAX_LISTA);
        assert!(processos.vigiados[0].nome.len() <= NOME_MAXIMO);

        let interesse = InteresseEmProcessos {
            nomes: vec![
                "  ".to_owned(),
                "a".to_owned(),
                "b".to_owned(),
                "c".to_owned(),
                "d".to_owned(),
                "e".to_owned(),
            ],
            lista: false,
        }
        .nos_tetos();
        assert_eq!(interesse.nomes, vec!["a", "b", "c", "d"]);
    }
}
