//! O que pode dar errado num pedido — e o que fazer a respeito.
//!
//! Separado dos pedidos e das respostas ([`crate::ui`]) porque é o vocabulário com mais regra
//! própria do contrato, e as duas regras valem para toda variante que entrar:
//!
//! 1. **Toda falha diz o que fazer agora** ([`Falha::o_que_fazer`]) — a terceira parte que
//!    [09, §5](../../../docs/09-padroes-de-codigo.md) exige e que a maioria dos produtos esquece.
//!    "Não deu" sem "e agora?" é a mensagem de erro que não ajuda ninguém.
//! 2. **Nenhuma falha muda de lugar.** O `postcard` grava a variante pelo índice, então interface e
//!    serviço de versões diferentes só se entendem se cada falha continuar onde nasceu. Variante
//!    nova entra no fim, nunca no meio — e um teste trava isso.

use serde::{Deserialize, Serialize};

/// O que pode dar errado num pedido.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, thiserror::Error)]
#[non_exhaustive]
pub enum Falha {
    /// Falta privilégio.
    #[error("esta ação precisa de permissão de administrador")]
    PrecisaElevacao,
    /// O pedido não faz sentido no estado atual.
    #[error("esta ação não faz sentido agora")]
    ForaDeContexto,
    /// O par indicado não existe.
    #[error("esse computador não está pareado")]
    ParDesconhecido,
    /// O pareamento expirou sem confirmação.
    #[error("o código expirou")]
    PareamentoExpirou,
    /// O usuário disse que os códigos não conferem.
    #[error("os códigos não conferiam")]
    CodigosDiferentes,
    /// Falha interna do serviço.
    #[error("falha interna do serviço")]
    Interna,
    // Daqui para baixo, as variantes que entraram depois — no fim, pela regra 2 do módulo.
    /// Este usuário não tem permissão para usar o serviço nesta máquina.
    #[error("este usuário não tem permissão para usar o InputRemote nesta máquina")]
    SemPermissao,
    /// A interface não conseguiu falar com o serviço.
    ///
    /// Não é o serviço que manda esta: é a interface que a produz quando o canal não abre ou caiu,
    /// para o pedido falhar com uma razão e uma instrução em vez de um "falha interna" genérico.
    #[error("o serviço do InputRemote não está respondendo")]
    ServicoIndisponivel,
    /// O papel pedido não funciona nesta plataforma.
    ///
    /// Hoje é o de servidor num Linux, que ainda não captura a entrada local. Aceitar a troca
    /// gravaria um papel em que nada funciona — e foi exatamente isso que aconteceu, em silêncio,
    /// antes desta falha existir.
    #[error("este computador ainda não pode ter o teclado e o mouse")]
    PapelIndisponivel,
    /// A borda só se escolhe no computador que tem o teclado e o mouse.
    ///
    /// O controlado usa sozinho a borda oposta à do outro. Deixar os dois escolherem foi o que
    /// deixou a bancada com os dois lados dizendo "esquerda" (log 24).
    #[error("a borda é escolhida no computador que tem o teclado e o mouse")]
    BordaDoServidor,
    /// O pareamento não chegou ao fim: o outro computador não respondeu, ou o código já não vale.
    ///
    /// Sem esta, um clique em "São iguais" num pareamento que já tinha caído não mudava nada na
    /// tela, e parecia que o botão não funcionava (log 25).
    #[error("o pareamento não chegou ao fim")]
    PareamentoInterrompido,
}

impl Falha {
    /// O que o usuário deve fazer agora.
    #[must_use]
    pub const fn o_que_fazer(self) -> &'static str {
        match self {
            Self::PrecisaElevacao => {
                "Feche e abra o InputRemote como administrador para concluir esta ação."
            }
            Self::ForaDeContexto => "Confira o estado da conexão e tente de novo.",
            Self::ParDesconhecido => "Pareie o computador antes de configurá-lo.",
            Self::PareamentoExpirou => "Comece o pareamento de novo; o código vale 2 minutos.",
            Self::CodigosDiferentes => {
                "Códigos diferentes significam que alguém pode estar no meio da conexão. \
                 Não pareie por esta rede e procure ajuda."
            }
            Self::Interna => {
                "Exporte o diagnóstico em Preferências e abra um relato com ele anexado."
            }
            Self::SemPermissao => {
                "Peça a um administrador para incluir seu usuário no grupo inputremote desta \
                 máquina. Vale na hora, sem reiniciar."
            }
            Self::ServicoIndisponivel => {
                "Confira se o serviço do InputRemote está em execução. A janela reconecta sozinha \
                 quando ele voltar."
            }
            Self::PapelIndisponivel => {
                "Neste computador, por enquanto, só funciona o papel de controlado. Use o outro \
                 computador como o que tem o teclado e o mouse."
            }
            Self::BordaDoServidor => {
                "Troque a borda no computador que tem o teclado e o mouse. Este acompanha sozinho, \
                 sem reconectar."
            }
            Self::PareamentoInterrompido => {
                "Comece o pareamento de novo por um dos computadores e compare o código novo nas \
                 duas telas."
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toda_falha_diz_o_que_fazer() {
        let falhas = [
            Falha::PrecisaElevacao,
            Falha::ForaDeContexto,
            Falha::ParDesconhecido,
            Falha::PareamentoExpirou,
            Falha::CodigosDiferentes,
            Falha::Interna,
            Falha::SemPermissao,
            Falha::ServicoIndisponivel,
            Falha::PapelIndisponivel,
            Falha::BordaDoServidor,
            Falha::PareamentoInterrompido,
        ];
        for falha in falhas {
            assert!(!falha.to_string().is_empty(), "{falha:?} sem descrição");
            let acao = falha.o_que_fazer();
            assert!(!acao.is_empty(), "{falha:?} não diz o que fazer");
            // Uma instrução tem verbo. É o mínimo para ser acionável.
            assert!(
                acao.len() > 20,
                "{falha:?}: `{acao}` é curto demais para instruir"
            );
        }
    }

    #[test]
    fn o_indice_de_cada_falha_nao_muda() {
        // Sem este teste, uma reordenação "cosmética" trocaria o significado das falhas no fio
        // entre versões diferentes, e nenhum outro teste perceberia: dentro de uma mesma versão
        // os dois lados concordam, errado, do mesmo jeito.
        let esperado = [
            (Falha::PrecisaElevacao, 0u8),
            (Falha::ForaDeContexto, 1),
            (Falha::ParDesconhecido, 2),
            (Falha::PareamentoExpirou, 3),
            (Falha::CodigosDiferentes, 4),
            (Falha::Interna, 5),
            (Falha::SemPermissao, 6),
            (Falha::ServicoIndisponivel, 7),
            (Falha::PapelIndisponivel, 8),
            (Falha::BordaDoServidor, 9),
            (Falha::PareamentoInterrompido, 10),
        ];
        for (falha, indice) in esperado {
            let bytes = postcard::to_allocvec(&falha).expect("serializa");
            assert_eq!(bytes, vec![indice], "{falha:?} mudou de lugar no fio");
        }
    }

    #[test]
    fn sem_permissao_nao_manda_reiniciar() {
        // A permissão é conferida na hora da conexão. Mandar reiniciar seria ensinar de novo a
        // instrução errada que custou uma tarde de "continua do mesmo jeito".
        let acao = Falha::SemPermissao.o_que_fazer();
        assert!(acao.contains("sem reiniciar"), "{acao}");
    }

    #[test]
    fn codigos_diferentes_avisa_do_risco_em_vez_de_so_pedir_para_repetir() {
        // Códigos diferentes é o sinal de homem no meio. Dizer "tente de novo" ensinaria o
        // usuário a insistir exatamente onde ele não deveria.
        let texto = Falha::CodigosDiferentes.o_que_fazer();
        assert!(
            texto.contains("meio"),
            "o risco precisa estar dito: `{texto}`"
        );
    }
}
