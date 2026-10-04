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
//! - no Linux, `linux::avisar` (o `notify-send`, só o fim: o andamento é da bandeja e do dock),
//!   usado pelo ajudante de clipboard, pelas pastas e pela janela;
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

use std::path::Path;

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

impl Tom {
    /// O tom de uma cópia: anda, deu certo, ou não atravessou. O mesmo para o recado e o ícone.
    #[must_use]
    pub const fn da_copia(copia: &Transferencia) -> Self {
        if copia.falhou() {
            Self::Problema
        } else if copia.terminou() {
            Self::Feito
        } else {
            Self::Andamento
        }
    }
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
    /// O que chegou, com o caminho inteiro: a miniatura de uma imagem, e o gerenciador de arquivos
    /// abrindo a pasta com ele já selecionado.
    pub recebido: Option<String>,
}

impl Recado {
    /// O recado de uma cópia de arquivos, em qualquer fase.
    #[must_use]
    pub fn da_copia(copia: &Transferencia) -> Self {
        let tom = Tom::da_copia(copia);
        let recebido = match (&copia.fase, copia.sentido) {
            (Fase::Concluida { destino }, Sentido::Recebendo) => Some(destino.clone()),
            _ => None,
        };
        let pasta = recebido.as_deref().and_then(|destino| {
            Path::new(destino)
                .parent()
                .map(|pasta| pasta.to_string_lossy().into_owned())
        });
        Self {
            titulo: copia.titulo().to_owned(),
            corpo: copia.detalhe(),
            tom,
            andamento: copia.em_curso().then(|| copia.progresso()),
            pasta,
            recebido,
        }
    }

    /// O que chegou, se é uma imagem: o recado a mostra em miniatura, e quem copiou uma captura de
    /// tela reconhece na hora o que chegou.
    #[must_use]
    pub fn imagem(&self) -> Option<&str> {
        let caminho = self.recebido.as_deref()?;
        let extensao = Path::new(caminho)
            .extension()?
            .to_str()?
            .to_ascii_lowercase();
        matches!(
            extensao.as_str(),
            "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp"
        )
        .then_some(caminho)
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
            recebido: None,
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
        assert_eq!(chegou.recebido.as_deref(), destino.to_str());
        assert_eq!(chegou.imagem(), None, "uma pasta não é imagem");
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

    #[test]
    fn a_imagem_que_chegou_vai_em_miniatura() {
        let fase = Fase::Concluida {
            destino: "/home/ana/Recebidos/Captura.PNG".to_owned(),
        };
        let chegou = Recado::da_copia(&copia(Sentido::Recebendo, fase));
        assert_eq!(chegou.imagem(), Some("/home/ana/Recebidos/Captura.PNG"));
    }
}
