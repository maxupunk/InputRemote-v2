//! O canal do agente — separado do da interface, com permissões próprias.
//!
//! A separação é requisito de segurança, não organização: **a interface nunca pode pedir
//! injeção de entrada**. Se qualquer processo do usuário pudesse mandar `Injetar` para o
//! serviço, qualquer programa que o usuário rodasse poderia digitar no prompt de UAC — e o
//! modelo de segurança do Windows na máquina cairia junto
//! ([04, §5](../../../docs/04-seguranca.md)).
//!
//! Por isso são dois transportes distintos, com descritores de segurança distintos, e dois
//! vocabulários que não se misturam. `Injetar` não existe em [`crate::ui::Pedido`], e é isso
//! que torna a garantia estrutural em vez de combinada.

use serde::{Deserialize, Serialize};

use ir_proto::input::{Button, HidUsage, PointerPosition, WheelDelta};

/// O que o serviço manda ao agente.
///
/// Comandos já resolvidos. O agente **não toma decisão nenhuma**
/// ([02, §1.2](../../../docs/02-arquitetura.md)): ele recebe "injete isto" e devolve "isto
/// aconteceu". Toda política vive no serviço, que é o que permite o agente morrer e ressubir
/// sem consequência.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum ComandoDoAgente {
    /// Injete esta tecla.
    Tecla {
        /// Qual.
        usage: HidUsage,
        /// `true` para pressionar.
        pressionada: bool,
    },
    /// Injete este botão.
    Botao {
        /// Qual.
        botao: Button,
        /// `true` para pressionar.
        pressionado: bool,
    },
    /// Injete este movimento de roda.
    Roda(WheelDelta),
    /// Ponha o ponteiro aqui.
    Ponteiro(PointerPosition),
    /// Solte tudo, agora.
    ///
    /// O comando mais importante do produto. O agente o obedece antes de qualquer coisa que
    /// esteja na fila.
    SoltarTudo,
    /// Ligue ou desligue a supressão da entrada local.
    SuprimirEntradaLocal(bool),
    /// Prenda o ponteiro local neste ponto.
    PrenderPonteiro(PointerPosition),
    /// Gere Ctrl+Alt+Del.
    ///
    /// Exige `SendSAS` e a política do sistema habilitada
    /// ([05, §4.3](../../../docs/05-windows.md)). Vai pelo canal do agente porque é ele quem
    /// sabe se o desktop de entrada é o seguro.
    SequenciaDeAtencao,
    /// Encerre.
    Encerrar,
}

/// O que o agente conta ao serviço.
///
/// Fatos, nunca decisões.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum FatoDoAgente {
    /// Estou pronto para injetar.
    Pronto {
        /// Em quais desktops as threads foram amarradas, para o diagnóstico.
        desktops: Vec<String>,
    },
    /// O desktop que está recebendo entrada mudou.
    ///
    /// No Windows acontece a cada bloqueio de tela e a cada prompt de UAC. Não existe
    /// notificação do sistema para isto, então o agente descobre consultando
    /// ([00b, §2](../../../docs/00-licoes-do-deskflow.md)).
    DesktopMudou {
        /// O nome do desktop que passou a receber entrada.
        nome: String,
    },
    /// Uma tecla local mudou de estado.
    TeclaLocal {
        /// Qual.
        usage: HidUsage,
        /// `true` para pressionada.
        pressionada: bool,
    },
    /// Um botão local mudou de estado.
    BotaoLocal {
        /// Qual.
        botao: Button,
        /// `true` para pressionado.
        pressionado: bool,
    },
    /// O ponteiro local se moveu.
    PonteiroLocal {
        /// Deslocamento horizontal.
        dx: i32,
        /// Deslocamento vertical.
        dy: i32,
    },
    /// A roda local girou.
    RodaLocal(WheelDelta),
    /// O usuário acionou o atalho de emergência.
    Emergencia,
    /// O arranjo de telas mudou.
    TelasMudaram(ir_proto::screens::ScreenLayout),
    /// A injeção foi recusada pelo sistema.
    ///
    /// É o sintoma do endurecimento de janeiro de 2026 quando as três origens confiáveis não
    /// são atendidas ([05, §4.4](../../../docs/05-windows.md)). O agente **precisa** contar,
    /// porque do ponto de vista do usuário nada aconteceu e ele não teria como saber.
    InjecaoRecusada {
        /// Em qual desktop.
        desktop: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn o_agente_relata_fatos_e_nao_faz_pedidos() {
        // O vocabulário do agente para o serviço é de fatos. Se alguém acrescentar um pedido
        // aqui, a política sai do serviço e o agente passa a decidir — que é o que
        // docs/02 §1.2 proíbe, e o que tornaria o agente não-descartável.
        let fatos = [
            FatoDoAgente::Emergencia,
            FatoDoAgente::DesktopMudou {
                nome: "Winlogon".to_owned(),
            },
            FatoDoAgente::InjecaoRecusada {
                desktop: "Winlogon".to_owned(),
            },
        ];
        for fato in fatos {
            let nome = format!("{fato:?}");
            assert!(
                !nome.contains("Pedir") && !nome.contains("Solicitar"),
                "`{nome}` parece um pedido, e o agente não pede"
            );
        }
    }

    #[test]
    fn soltar_tudo_existe_no_vocabulario_do_agente() {
        // É o comando que toda falha emite. Sem ele no contrato, o serviço não teria como
        // cumprir a promessa de que nada fica pressionado.
        assert_eq!(ComandoDoAgente::SoltarTudo, ComandoDoAgente::SoltarTudo);
    }

    #[test]
    fn a_recusa_de_injecao_diz_em_qual_desktop() {
        // Sem o desktop, o diagnóstico não distingue "recusou na tela de bloqueio" de
        // "recusou no desktop normal" — que são problemas completamente diferentes.
        let fato = FatoDoAgente::InjecaoRecusada {
            desktop: "Winlogon".to_owned(),
        };
        match fato {
            FatoDoAgente::InjecaoRecusada { desktop } => assert_eq!(desktop, "Winlogon"),
            _ => panic!("variante errada"),
        }
    }
}
