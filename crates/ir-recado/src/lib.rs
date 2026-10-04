//! O recado fora da janela: a notificação do sistema.
//!
//! A janela do InputRemote fica fechada quase sempre — quem copia está no Explorer ou no Nautilus.
//! O retorno do que acontece (a cópia que anda, a que chegou, a que não atravessou, a pasta que o
//! outro ofereceu) vai para onde o sistema põe os recados de todo programa: a central de
//! notificações. Ali ele respeita o "não perturbe", fica no histórico e tem a cara do sistema.
//!
//! **O que dizer** e **quando** moram aqui, uma vez só, para os dois sistemas dizerem o mesmo:
//! [`Recado`] e [`Ritmo`]. **Como mostrar** é de cada um:
//!
//! - no Linux, `linux::NotifySend` (o `notify-send`), usado pelo ajudante de clipboard, pelas pastas
//!   e pela janela;
//! - no Windows, `central::Notificacoes` (a central de notificações), usada pela interface, que mora
//!   na bandeja.
//!
//! E o ícone da bandeja do Windows ([`bandeja`]), o outro retorno que se vê sem abrir a janela.
//!
//! # Por que um crate
//!
//! Nasceu de uma fronteira comprovada: a interface passou de 2 500 linhas de produção no dia em que
//! a notificação nativa e o ícone com estados entraram nela, e o `notify-send` estava repetido em
//! quatro lugares de três crates ([log 64](../../../docs/logs/64-copiar-e-colar-nos-dois.md)).
//! Depende só do vocabulário de `ir-ipc`: não conhece o produto, só sabe dar recado.

#![cfg_attr(
    test,
    allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::indexing_slicing,
        clippy::panic
    )
)]

pub mod bandeja;
#[cfg(windows)]
pub mod central;
#[cfg(target_os = "linux")]
pub mod linux;
mod ritmo;

pub use ritmo::{Ritmo, Vez};

use ir_ipc::transferencia::{Fase, Sentido, Transferencia};

/// O tom do recado: muda o ícone de estado, o som e a urgência, conforme o sistema.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tom {
    /// Algo acontecendo agora, com andamento.
    Andamento,
    /// Terminou bem.
    Feito,
    /// Não deu certo, e pede atenção de quem lê.
    Problema,
    /// Uma informação: nada deu errado, mas a pessoa precisa saber.
    Informacao,
}

/// Um recado pronto para a central de notificações.
#[derive(Debug, Clone, PartialEq)]
pub struct Recado {
    /// A primeira linha.
    pub titulo: String,
    /// O resto, numa linha ou duas.
    pub corpo: String,
    /// O tom.
    pub tom: Tom,
    /// O andamento, de 0 a 1, enquanto anda — o sistema que sabe desenhar barra desenha.
    pub andamento: Option<f32>,
    /// Uma pasta deste computador para o botão "Abrir a pasta": onde o que chegou ficou.
    pub pasta: Option<String>,
}

impl Recado {
    /// O recado de uma cópia de arquivos, em qualquer fase.
    #[must_use]
    pub fn da_copia(copia: &Transferencia) -> Self {
        let tom = if copia.falhou() {
            Tom::Problema
        } else if copia.terminou() {
            Tom::Feito
        } else {
            Tom::Andamento
        };
        let pasta = match (&copia.fase, copia.sentido) {
            (Fase::Concluida { destino }, Sentido::Recebendo) => std::path::Path::new(destino)
                .parent()
                .map(|pasta| pasta.to_string_lossy().into_owned()),
            _ => None,
        };
        Self {
            titulo: copia.titulo().to_owned(),
            corpo: copia.detalhe(),
            tom,
            andamento: copia.em_curso().then(|| copia.progresso()),
            pasta,
        }
    }

    /// O outro computador quer compartilhar uma pasta com este.
    #[must_use]
    pub fn oferta_de_pasta(par: &str, nome: &str) -> Self {
        Self::informacao(
            format!("{} quer compartilhar a pasta \"{nome}\"", quem(par)),
            "Abra o InputRemote, em Pastas compartilhadas, para aceitar.".to_owned(),
        )
    }

    /// A conexão com o outro computador caiu, com o que se sabe do motivo.
    #[must_use]
    pub fn conexao_perdida(detalhe: &str) -> Self {
        Self {
            tom: Tom::Problema,
            ..Self::informacao("Conexão perdida".to_owned(), detalhe.to_owned())
        }
    }

    /// O outro computador ligou ou desligou "copiar e colar", e este acompanhou.
    ///
    /// Sem este recado, quem está aqui veria o Ctrl+C parar de atravessar sem ter mexido em nada.
    #[must_use]
    pub fn copiar_e_colar_ajustado(ligado: bool, par: &str) -> Self {
        let onde = if par.is_empty() {
            "no outro computador".to_owned()
        } else {
            format!("em {par}")
        };
        if ligado {
            Self::informacao(
                "Copiar e colar ligado".to_owned(),
                format!(
                    "Ligado {onde}, e vale para os dois: o que se copia num computador fica \
                     pronto para colar no outro."
                ),
            )
        } else {
            Self::informacao(
                "Copiar e colar desligado".to_owned(),
                format!(
                    "Desligado {onde}, e vale para os dois: nada do que se copia passa de um \
                     computador ao outro. Para religar, Preferências."
                ),
            )
        }
    }

    const fn informacao(titulo: String, corpo: String) -> Self {
        Self {
            titulo,
            corpo,
            tom: Tom::Informacao,
            andamento: None,
            pasta: None,
        }
    }
}

/// O nome do outro computador numa frase, ou "O outro computador" quando ele não é conhecido.
fn quem(par: &str) -> &str {
    if par.is_empty() {
        "O outro computador"
    } else {
        par
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ir_ipc::transferencia::Motivo;

    fn copia(sentido: Sentido, fase: Fase) -> Transferencia {
        Transferencia {
            sentido,
            nome: "fotos".to_owned(),
            bytes_feitos: 50,
            bytes_total: 100,
            fase,
        }
    }

    #[test]
    fn a_copia_que_anda_leva_o_andamento_e_a_que_terminou_nao() {
        let andando = Recado::da_copia(&copia(Sentido::Enviando, Fase::Andando));
        assert_eq!(andando.tom, Tom::Andamento);
        assert_eq!(andando.andamento, Some(0.5));
        let parada = Recado::da_copia(&copia(Sentido::Enviando, Fase::Parada(Motivo::CanalCaiu)));
        assert_eq!(parada.tom, Tom::Problema);
        assert_eq!(parada.andamento, None);
    }

    #[test]
    fn so_o_que_chegou_aqui_oferece_abrir_a_pasta() {
        let destino = std::path::Path::new("recebidos").join("fotos");
        let fase = Fase::Concluida {
            destino: destino.to_string_lossy().into_owned(),
        };
        let chegou = Recado::da_copia(&copia(Sentido::Recebendo, fase.clone()));
        assert_eq!(chegou.tom, Tom::Feito);
        assert_eq!(chegou.pasta.as_deref(), Some("recebidos"));
        let foi = Recado::da_copia(&copia(Sentido::Enviando, fase));
        assert_eq!(foi.pasta, None, "o que foi está lá, e não aqui");
    }

    #[test]
    fn o_ajuste_de_copiar_e_colar_diz_onde_e_que_vale_para_os_dois() {
        let desligado = Recado::copiar_e_colar_ajustado(false, "NOTEBOOK");
        assert_eq!(desligado.titulo, "Copiar e colar desligado");
        assert!(
            desligado.corpo.contains("em NOTEBOOK"),
            "{}",
            desligado.corpo
        );
        assert!(desligado.corpo.contains("vale para os dois"));
        let sem_nome = Recado::copiar_e_colar_ajustado(true, "");
        assert!(sem_nome.corpo.contains("no outro computador"));
    }

    #[test]
    fn a_oferta_diz_quem_e_o_que_fazer() {
        let oferta = Recado::oferta_de_pasta("NOTEBOOK", "Fotos");
        assert_eq!(
            oferta.titulo,
            "NOTEBOOK quer compartilhar a pasta \"Fotos\""
        );
        assert_eq!(
            Recado::oferta_de_pasta("", "Fotos").titulo,
            "O outro computador quer compartilhar a pasta \"Fotos\""
        );
    }
}
