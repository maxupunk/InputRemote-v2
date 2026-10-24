//! O que o painel recebe além do quadro: a cópia de arquivos em curso e os processos.
//!
//! Vai num recado próprio ([`ParaPainel::Detalhes`](crate::ParaPainel::Detalhes)), ao lado do
//! quadro de cada segundo, e só para o painel que disse o que quer ([`Interesse`]) — um aplicativo
//! de antes desta versão nunca o recebe, e o quadro dele segue igual.

use serde::{Deserialize, Serialize};

use crate::processos::{InteresseEmProcessos, Processos};

/// O que o painel quer além do quadro.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Interesse {
    /// Os processos vigiados — os cards de processo do painel.
    pub processos: Vec<String>,
    /// De qual computador do quadro (pela posição) a lista de processos está aberta.
    pub lista: Option<u8>,
}

impl Interesse {
    /// O que pedir a cada computador, juntando os painéis: os vigiados valem para os dois, e a
    /// lista só para o computador em que alguém a abriu. A posição é a do quadro: 0 é este
    /// computador, 1 o par.
    #[must_use]
    pub fn por_maquina<'a>(
        interesses: impl IntoIterator<Item = &'a Self>,
    ) -> [InteresseEmProcessos; 2] {
        let mut maquinas = [
            InteresseEmProcessos::default(),
            InteresseEmProcessos::default(),
        ];
        for interesse in interesses {
            for (posicao, maquina) in (0u8..).zip(maquinas.iter_mut()) {
                *maquina = maquina.juntar(&InteresseEmProcessos {
                    nomes: interesse.processos.clone(),
                    lista: interesse.lista == Some(posicao),
                });
            }
        }
        maquinas
    }
}

/// O que o painel recebe além do quadro, neste segundo.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Detalhes {
    /// A cópia de arquivos entre os computadores, em curso ou que acabou há pouco.
    pub copia: Option<Copia>,
    /// Os processos de cada computador, na ordem do quadro; `None` de quem não os mandou.
    pub processos: Vec<Option<Processos>>,
}

/// Uma cópia de arquivos entre os dois computadores.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Copia {
    /// `true` saindo do computador que agrega; `false` chegando a ele.
    pub enviando: bool,
    /// O nome do que está indo — o do arquivo ou da pasta copiada, nunca o caminho.
    pub nome: String,
    /// Bytes já copiados.
    pub feitos: u64,
    /// Bytes no total.
    pub total: u64,
    /// A velocidade, suavizada; `None` até haver duas medidas.
    pub por_segundo: Option<u64>,
    /// Em que pé está.
    pub fase: FaseDaCopia,
}

/// Em que pé está uma cópia. A ordem das variantes é o formato de fio.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum FaseDaCopia {
    /// Anunciada, esperando o outro lado aceitar.
    Esperando,
    /// Em curso.
    Andando,
    /// Chegou inteira.
    Concluida,
    /// Parou, com a frase do motivo.
    Parada(String),
    /// Parada à espera da conexão de arquivos; segue sozinha quando ela voltar.
    AguardandoConexao,
}

impl Copia {
    /// Se ainda está acontecendo.
    #[must_use]
    pub const fn em_curso(&self) -> bool {
        matches!(
            self.fase,
            FaseDaCopia::Esperando | FaseDaCopia::Andando | FaseDaCopia::AguardandoConexao
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn os_vigiados_valem_para_os_dois_e_a_lista_so_onde_foi_aberta() {
        let tablet = Interesse {
            processos: vec!["docker".to_owned()],
            lista: Some(1),
        };
        let celular = Interesse {
            processos: vec!["wsl".to_owned(), "Docker".to_owned()],
            lista: None,
        };
        let [daqui, par] = Interesse::por_maquina([&tablet, &celular]);
        assert_eq!(daqui.nomes, vec!["docker", "wsl"]);
        assert!(!daqui.lista);
        assert_eq!(par.nomes, daqui.nomes);
        assert!(par.lista);
        assert!(
            Interesse::por_maquina([])
                .iter()
                .all(InteresseEmProcessos::vazio)
        );
    }
}
