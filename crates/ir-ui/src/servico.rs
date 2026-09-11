//! O que a interface precisa de um serviço — e nada além disso.
//!
//! A interface fala com um `dyn Servico`, não com um socket. Assim a janela é desenvolvida,
//! avaliada e testada contra [`crate::simulado::ServicoSimulado`], e roda de verdade contra
//! [`crate::real::ServicoReal`], sem que nada nela mude.
//!
//! O contrato é pequeno de propósito. Se ele crescer, é sinal de que lógica de produto está
//! vazando para cá — que é exatamente como o v1 acabou com 10.491 linhas no crate da interface.

use ir_ipc::{Autoridade, Aviso, Pedido, Resposta};

/// Um serviço com que a interface consegue conversar.
pub trait Servico {
    /// Faz um pedido e espera a resposta.
    ///
    /// Síncrono porque a interface roda numa thread só e todo pedido é local e curto. Um pedido
    /// que precise demorar — descoberta, pareamento — responde [`Resposta::Feito`] na hora e
    /// conta o resultado depois por [`Servico::avisos`].
    fn pedir(&self, pedido: Pedido) -> Resposta;

    /// Recolhe o que o serviço tem a contar desde a última consulta.
    ///
    /// A interface consulta em intervalo fixo. Fila, e não retorno único, porque perder um aviso
    /// deixaria a tela mostrando um estado que já passou.
    fn avisos(&self) -> Vec<Aviso>;

    /// O privilégio que este processo tem.
    ///
    /// A interface usa para avisar **antes** que uma ação vai ser recusada, em vez de deixar o
    /// usuário descobrir depois de comparar seis dígitos.
    fn autoridade(&self) -> Autoridade;

    /// Em que pé está a ligação com o serviço, agora.
    ///
    /// A janela mostra isso na cara. Uma interface que finge estar funcionando é pior que uma que
    /// não abre — e uma que diz só "não funciona", sem dizer o que fazer, não é muito melhor.
    fn situacao(&self) -> Situacao;
}

/// A ligação da interface com o serviço.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Situacao {
    /// Falando com o serviço de verdade.
    Conectado,
    /// Falando com o serviço simulado, pedido de propósito para desenvolver ou demonstrar.
    Simulado,
    /// Sem o serviço agora, pelo motivo dado. A interface continua tentando.
    Desconectado(Desconexao),
}

/// Por que a interface não está falando com o serviço.
///
/// Só dois motivos, e não um por erro do sistema: o que interessa a quem olha a janela é o que
/// fazer, e há exatamente duas coisas diferentes a fazer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Desconexao {
    /// O serviço não está no ar, ou parou de responder.
    ServicoParado,
    /// O serviço está no ar, mas este usuário não tem permissão para usá-lo.
    SemPermissao,
}

impl Desconexao {
    /// O que aconteceu, em uma frase.
    #[must_use]
    pub const fn frase(self) -> &'static str {
        match self {
            Self::ServicoParado => {
                "O serviço do InputRemote não está respondendo. Nada é injetado em computador nenhum."
            }
            Self::SemPermissao => {
                "Este usuário não tem permissão para usar o InputRemote nesta máquina."
            }
        }
    }

    /// O que fazer a respeito.
    ///
    /// A instrução é da plataforma em que a janela está: um comando de Linux mostrado no Windows
    /// ensinaria a pessoa a desconfiar das instruções.
    #[must_use]
    pub const fn o_que_fazer(self) -> &'static str {
        match self {
            Self::ServicoParado if cfg!(windows) => {
                "Confira o serviço \"InputRemote\" em Serviços do Windows. Esta janela reconecta \
                 sozinha quando ele voltar."
            }
            Self::ServicoParado => {
                "Suba o serviço com \"sudo systemctl enable --now inputremote\". Esta janela \
                 reconecta sozinha quando ele voltar."
            }
            Self::SemPermissao if cfg!(windows) => {
                "Reinstale a versão atual do InputRemote como administrador. Esta janela reconecta \
                 sozinha quando a permissão estiver certa."
            }
            Self::SemPermissao => {
                "Peça a um administrador: \"sudo usermod -aG inputremote <seu usuário>\". Vale na \
                 hora, sem reiniciar, e esta janela reconecta sozinha."
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TODAS: [Desconexao; 2] = [Desconexao::ServicoParado, Desconexao::SemPermissao];

    #[test]
    fn toda_desconexao_diz_o_que_aconteceu_e_o_que_fazer() {
        for motivo in TODAS {
            assert!(!motivo.frase().is_empty(), "{motivo:?} sem frase");
            assert!(
                motivo.o_que_fazer().len() > 20,
                "{motivo:?}: instrução curta demais para instruir"
            );
        }
    }

    #[test]
    fn a_janela_promete_que_volta_sozinha_e_a_promessa_e_verdadeira() {
        // A reconexão existe (`crate::conexao`); dizer isso evita que a pessoa feche e reabra a
        // janela a cada queda, achando que é preciso.
        for motivo in TODAS {
            assert!(
                motivo.o_que_fazer().contains("sozinha"),
                "{motivo:?}: `{}`",
                motivo.o_que_fazer()
            );
        }
    }

    #[test]
    fn sem_permissao_no_linux_nao_manda_reiniciar() {
        // A permissão é conferida no banco de usuários na hora da conexão. Mandar reiniciar seria
        // repetir a instrução errada que a correção existiu para aposentar.
        if !cfg!(windows) {
            let instrucao = Desconexao::SemPermissao.o_que_fazer();
            assert!(instrucao.contains("sem reiniciar"), "{instrucao}");
            assert!(instrucao.contains("usermod"), "{instrucao}");
        }
    }
}
