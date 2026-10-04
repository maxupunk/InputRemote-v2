//! As frases das pastas compartilhadas, como a janela as mostra.
//!
//! Escritas aqui, e não no `.slint`, pela regra da casa: uma frase errada está num arquivo só, e
//! tem teste. E cada situação que pede algo da pessoa diz o quê — "sem conexão" sozinho não ajuda.

use super::{PapelDaPasta, ResumoDePasta, SituacaoDaPasta};

impl ResumoDePasta {
    /// A situação, em uma frase.
    #[must_use]
    pub fn linha(&self) -> String {
        match (self.situacao, self.papel) {
            (SituacaoDaPasta::Oferecida, PapelDaPasta::Recebida) => {
                "O outro computador quer compartilhar esta pasta com este.".to_owned()
            }
            (SituacaoDaPasta::Oferecida, PapelDaPasta::Compartilhada) => {
                "Esperando o outro computador aceitar.".to_owned()
            }
            (SituacaoDaPasta::EmDia, _) => "Em dia nos dois computadores.".to_owned(),
            (SituacaoDaPasta::Sincronizando, _) if self.pendentes > 0 => {
                format!("Enviando {}…", mudancas(self.pendentes))
            }
            (SituacaoDaPasta::Sincronizando, _) if self.baixando > 0 => {
                format!("Recebendo {}…", arquivos(self.baixando))
            }
            (SituacaoDaPasta::Sincronizando, _) => "Sincronizando…".to_owned(),
            (SituacaoDaPasta::SemConexao, _) if self.pendentes > 0 => format!(
                "O outro computador não está ao alcance. {} esperando; vão sozinhas quando ele voltar.",
                maiuscula(&mudancas(self.pendentes))
            ),
            (SituacaoDaPasta::SemConexao, _) => {
                "O outro computador não está ao alcance. O que mudar aqui vai sozinho quando ele voltar."
                    .to_owned()
            }
            (SituacaoDaPasta::ParDesatualizado, _) => {
                "Atualize o InputRemote no outro computador para esta pasta sincronizar.".to_owned()
            }
        }
    }

    /// De onde a pasta veio e onde ela está.
    #[must_use]
    pub fn detalhe(&self) -> String {
        let papel = match self.papel {
            PapelDaPasta::Compartilhada => "Compartilhada por este computador",
            PapelDaPasta::Recebida => "Recebida do outro computador",
        };
        if self.caminho_local.is_empty() {
            papel.to_owned()
        } else {
            format!("{papel} · {}", self.caminho_local)
        }
    }

    /// Os conflitos, quando há: o que aconteceu e que nada se perdeu.
    #[must_use]
    pub fn sobre_conflitos(&self) -> String {
        match self.conflitos {
            0 => String::new(),
            1 => "Um arquivo foi mudado nos dois computadores ao mesmo tempo. As duas versões \
                  foram guardadas, lado a lado."
                .to_owned(),
            n => format!(
                "{n} arquivos foram mudados nos dois computadores ao mesmo tempo. As duas versões \
                 de cada um foram guardadas, lado a lado."
            ),
        }
    }

    /// 0 em dia, 1 andando, 2 pede atenção — a mesma escala do estado da janela.
    #[must_use]
    pub const fn saude(&self) -> i32 {
        match self.situacao {
            _ if self.conflitos > 0 => 2,
            SituacaoDaPasta::EmDia => 0,
            SituacaoDaPasta::Sincronizando | SituacaoDaPasta::Oferecida => 1,
            _ => 2,
        }
    }
}

/// Uma linha para a tela inicial sobre todas as pastas. Vazia quando não há nenhuma.
#[must_use]
pub fn resumo_das_pastas(pastas: &[ResumoDePasta]) -> String {
    let ofertas = pastas
        .iter()
        .filter(|p| p.situacao == SituacaoDaPasta::Oferecida && p.papel == PapelDaPasta::Recebida)
        .count();
    if ofertas > 0 {
        return if ofertas == 1 {
            "O outro computador quer compartilhar uma pasta com este.".to_owned()
        } else {
            format!("O outro computador quer compartilhar {ofertas} pastas com este.")
        };
    }
    let total = pastas.len();
    if total == 0 {
        return String::new();
    }
    let conflitos: u32 = pastas.iter().map(|p| p.conflitos).sum();
    let quantas = if total == 1 {
        "1 pasta".to_owned()
    } else {
        format!("{total} pastas")
    };
    let como = if conflitos > 0 {
        "com versões duplas para conferir"
    } else if pastas.iter().all(|p| p.situacao == SituacaoDaPasta::EmDia) {
        "em dia"
    } else if pastas
        .iter()
        .any(|p| p.situacao == SituacaoDaPasta::Sincronizando)
    {
        "sincronizando"
    } else {
        "esperando o outro computador"
    };
    format!("{quantas} · {como}")
}

fn arquivos(n: u32) -> String {
    if n == 1 {
        "1 arquivo".to_owned()
    } else {
        format!("{n} arquivos")
    }
}

/// O que dizer de um conflito, em uma frase: o nome do arquivo e onde está a outra versão.
#[must_use]
pub fn frase_do_conflito(conflito: &super::ConflitoDePasta) -> String {
    let nome = |caminho: &str| caminho.rsplit('/').next().unwrap_or(caminho).to_owned();
    format!(
        "{}: a versão mais recente ficou com o nome; a outra está em \"{}\".",
        nome(&conflito.original),
        nome(&conflito.copia)
    )
}

fn mudancas(n: u32) -> String {
    if n == 1 {
        "1 mudança".to_owned()
    } else {
        format!("{n} mudanças")
    }
}

fn maiuscula(texto: &str) -> String {
    let mut letras = texto.chars();
    letras.next().map_or_else(String::new, |primeira| {
        primeira.to_uppercase().chain(letras).collect()
    })
}

#[cfg(test)]
mod testes {
    use super::*;
    use crate::pastas::IdDePasta;

    fn pasta(situacao: SituacaoDaPasta, papel: PapelDaPasta, pendentes: u32) -> ResumoDePasta {
        ResumoDePasta {
            id: IdDePasta([1; 16]),
            nome: "Projetos".into(),
            caminho_local: "/home/ana/InputRemote/Projetos".into(),
            papel,
            situacao,
            pendentes,
            conflitos: 0,
            baixando: 0,
            lista_de_conflitos: Vec::new(),
            lixeira_com_algo: false,
        }
    }

    #[test]
    fn offline_a_frase_conta_o_que_espera_e_que_vai_sozinho() {
        let p = pasta(SituacaoDaPasta::SemConexao, PapelDaPasta::Recebida, 3);
        assert_eq!(
            p.linha(),
            "O outro computador não está ao alcance. 3 mudanças esperando; vão sozinhas quando ele voltar."
        );
        assert_eq!(p.saude(), 2);
    }

    #[test]
    fn a_oferta_diz_quem_espera_quem() {
        let recebida = pasta(SituacaoDaPasta::Oferecida, PapelDaPasta::Recebida, 0);
        assert!(recebida.linha().contains("quer compartilhar"));
        let compartilhada = pasta(SituacaoDaPasta::Oferecida, PapelDaPasta::Compartilhada, 0);
        assert!(compartilhada.linha().contains("aceitar"));
        assert_eq!(
            resumo_das_pastas(&[recebida]),
            "O outro computador quer compartilhar uma pasta com este."
        );
    }

    #[test]
    fn conflito_diz_que_nada_se_perdeu() {
        let mut p = pasta(SituacaoDaPasta::EmDia, PapelDaPasta::Compartilhada, 0);
        p.conflitos = 2;
        assert!(p.sobre_conflitos().contains("As duas versões"));
        assert_eq!(p.saude(), 2, "conflito pede atenção mesmo em dia");
        assert_eq!(
            resumo_das_pastas(&[p]),
            "1 pasta · com versões duplas para conferir"
        );
    }

    #[test]
    fn a_tela_inicial_resume_todas() {
        let em_dia = pasta(SituacaoDaPasta::EmDia, PapelDaPasta::Recebida, 0);
        assert_eq!(
            resumo_das_pastas(&[em_dia.clone(), em_dia]),
            "2 pastas · em dia"
        );
        assert_eq!(resumo_das_pastas(&[]), "");
    }
}
