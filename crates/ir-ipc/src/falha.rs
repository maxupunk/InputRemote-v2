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
    /// A política pedida não funciona nesta máquina: "só este controla", sem ler o próprio teclado.
    ///
    /// Aceitar gravaria uma política em que nada funciona — e foi exatamente isso que aconteceu, em
    /// silêncio, antes desta falha existir (quando ainda era um papel).
    #[error("este computador não consegue controlar o outro")]
    PoliticaIndisponivel,
    /// Não é mais produzida: desde o controle simétrico (ADR-0014), a borda se escolhe dos dois
    /// lados. Fica aqui pela regra 2 do módulo — tirá-la mudaria o índice das que vêm depois.
    #[error("a borda é escolhida no computador que tem o teclado e o mouse")]
    BordaDoServidor,
    /// O pareamento não chegou ao fim: o outro computador não respondeu, ou o código já não vale.
    ///
    /// Sem esta, um clique em "São iguais" num pareamento que já tinha caído não mudava nada na
    /// tela, e parecia que o botão não funcionava (log 25).
    #[error("o pareamento não chegou ao fim")]
    PareamentoInterrompido,
    /// Pediu-se o pareamento, e o outro computador não respondeu.
    ///
    /// Sem esta, a janela ficava em "Aguardando o outro computador" para sempre: o pedido saía, o
    /// outro lado não estava lá — serviço parado, endereço de outra rede, porta errada —, e nada
    /// voltava para dizer isso.
    #[error("o outro computador não respondeu")]
    ParNaoRespondeu,
    /// Pediu-se o Bluetooth, e este computador não tem rádio ligado.
    #[error("o Bluetooth deste computador está desligado ou não existe")]
    SemBluetooth,
    /// O pedido precisa dos dois computadores conectados, e eles não estão.
    #[error("os dois computadores não estão conectados agora")]
    SemConexao,
    /// O outro computador roda uma versão que não conhece este pedido.
    #[error("o outro computador tem uma versão mais antiga do InputRemote")]
    ParDesatualizado,
    /// O endereço digitado não é um `ip:porta` nem um endereço de Bluetooth.
    #[error("não entendi o endereço")]
    EnderecoInvalido,
    /// A ferramenta do sistema recusou a mudança.
    #[error("o sistema recusou a mudança")]
    SistemaRecusou,
    /// Pediu-se o teclado para este computador, e ele não consegue ler o teclado e o mouse locais.
    ///
    /// Sem esta, a troca era aceita e a máquina virava a que tem o teclado sem ter o que capturar:
    /// o ponteiro ia até a borda e nada acontecia (log 47).
    #[error("este computador não consegue ler o teclado e o mouse ligados a ele")]
    SemCaptura,
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
            Self::SemPermissao | Self::ServicoIndisponivel => self.o_que_fazer_sem_o_servico(),
            Self::PoliticaIndisponivel => {
                "Este computador não lê o próprio teclado e mouse, então não controla o outro. \
                 Escolha \"Os dois\" ou \"Só o outro controla este\"."
            }
            Self::BordaDoServidor => {
                "Troque a borda no computador que tem o teclado e o mouse. Este acompanha sozinho, \
                 sem reconectar."
            }
            Self::PareamentoInterrompido => {
                "Comece o pareamento de novo por um dos computadores e compare o código novo nas \
                 duas telas."
            }
            Self::ParNaoRespondeu => {
                "Abra o InputRemote no outro computador e confira se os dois estão na mesma rede. \
                 Se ele não aparecer na lista, digite o endereço dele."
            }
            _ => self.o_que_fazer_nas_mais_novas(),
        }
    }

    /// O que fazer quando a janela não alcança o serviço — a instrução da plataforma em que ela está.
    ///
    /// Um lugar só para a falha do pedido e para a faixa da janela desconectada: antes eram três
    /// textos, e o desta falha dava a instrução do Linux (o grupo `inputremote`) também no Windows,
    /// o que ensina a pessoa a desconfiar das instruções.
    const fn o_que_fazer_sem_o_servico(self) -> &'static str {
        match self {
            Self::SemPermissao if cfg!(windows) => {
                "Reinstale a versão atual do InputRemote como administrador. Esta janela reconecta \
                 sozinha quando a permissão estiver certa."
            }
            Self::SemPermissao => {
                "Peça a um administrador: \"sudo usermod -aG inputremote <seu usuário>\". Vale na \
                 hora, sem reiniciar, e esta janela reconecta sozinha."
            }
            _ if cfg!(windows) => {
                "Confira o serviço \"InputRemote\" em Serviços do Windows. Esta janela reconecta \
                 sozinha quando ele voltar."
            }
            _ => {
                "Suba o serviço com \"sudo systemctl enable --now inputremote\". Esta janela \
                 reconecta sozinha quando ele voltar."
            }
        }
    }

    /// O que fazer nas falhas que entraram depois da varredura de melhorias (log 45) — separadas só
    /// para cada função caber no limite de tamanho ([09, §1](../../../docs/09-padroes-de-codigo.md)).
    const fn o_que_fazer_nas_mais_novas(self) -> &'static str {
        match self {
            Self::SemBluetooth => {
                "Ligue o Bluetooth nas configurações do sistema, ou pareie pela rede local. O \
                 InputRemote percebe o rádio sozinho quando ele liga."
            }
            Self::SemConexao => {
                "Espere os dois computadores se conectarem de novo e tente outra vez. Se estiver \
                 pausado, retome primeiro."
            }
            Self::ParDesatualizado => {
                "Atualize o InputRemote no outro computador para a mesma versão deste."
            }
            Self::EnderecoInvalido => {
                "Use o IP do outro computador, como 192.168.0.10 (a porta é opcional), ou o \
                 endereço Bluetooth dele, como AC:50:DE:47:EB:28."
            }
            Self::SistemaRecusou => {
                "Veja o registro do serviço para o motivo, ou faça a mudança pelas configurações \
                 do sistema."
            }
            Self::SemCaptura => {
                "Instale a versão mais nova do InputRemote neste computador e tente de novo. \
                 Enquanto isso, deixe o teclado com o outro computador."
            }
            // As de cima já responderam em `o_que_fazer`.
            _ => "",
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
            Falha::PoliticaIndisponivel,
            Falha::BordaDoServidor,
            Falha::PareamentoInterrompido,
            Falha::ParNaoRespondeu,
            Falha::SemBluetooth,
            Falha::SemConexao,
            Falha::ParDesatualizado,
            Falha::EnderecoInvalido,
            Falha::SistemaRecusou,
            Falha::SemCaptura,
        ];
        for falha in falhas {
            assert!(!falha.to_string().is_empty(), "{falha:?} sem descrição");
            // A continuação de linha das frases longas já se perdeu em edição por script, e o
            // sintoma é um buraco de espaços no meio da frase.
            assert!(!falha.o_que_fazer().contains("  "), "{falha:?} tem buraco");
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
            (Falha::PoliticaIndisponivel, 8),
            (Falha::BordaDoServidor, 9),
            (Falha::PareamentoInterrompido, 10),
            (Falha::ParNaoRespondeu, 11),
            (Falha::SemBluetooth, 12),
            (Falha::SemConexao, 13),
            (Falha::ParDesatualizado, 14),
            (Falha::EnderecoInvalido, 15),
            (Falha::SistemaRecusou, 16),
            (Falha::SemCaptura, 17),
        ];
        for (falha, indice) in esperado {
            let bytes = postcard::to_allocvec(&falha).expect("serializa");
            assert_eq!(bytes, vec![indice], "{falha:?} mudou de lugar no fio");
        }
    }

    #[test]
    fn sem_permissao_da_a_instrucao_da_plataforma() {
        // A permissão é conferida na hora da conexão. Mandar reiniciar seria ensinar de novo a
        // instrução errada que custou uma tarde de "continua do mesmo jeito" — e mandar ao grupo
        // do Linux quem está no Windows ensinaria a desconfiar das instruções.
        let acao = Falha::SemPermissao.o_que_fazer();
        if cfg!(windows) {
            assert!(!acao.contains("usermod"), "{acao}");
            assert!(!acao.contains("grupo"), "{acao}");
        } else {
            assert!(acao.contains("sem reiniciar"), "{acao}");
            assert!(acao.contains("usermod"), "{acao}");
        }
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
