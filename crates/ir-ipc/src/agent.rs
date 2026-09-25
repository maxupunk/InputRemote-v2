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

use ir_proto::input::{Capture, Injection, PointerPosition};

/// O que o serviço manda ao agente.
///
/// Comandos já resolvidos. O agente **não toma decisão nenhuma**
/// ([02, §1.2](../../../docs/02-arquitetura.md)): ele recebe "injete isto" e devolve "isto
/// aconteceu". Toda política vive no serviço, que é o que permite o agente morrer e ressubir
/// sem consequência.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum ComandoDoAgente {
    /// Injete isto: tecla, botão, roda ou ponteiro.
    ///
    /// O tipo é o que a sessão pede e o injetor recebe, sem tradução no caminho: eram três enums e
    /// duas traduções, cada uma com um curinga que descartaria calado uma variante nova.
    Injetar(Injection),
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
    /// Se o par pode digitar nos desktops protegidos: a tela de bloqueio e o UAC.
    ///
    /// Desligado até o serviço dizer o contrário. Com ele desligado, o agente recusa injetar
    /// fora da área de trabalho e conta a recusa — quem está do outro lado precisa saber por que
    /// o teclado parou ([04, §6](../../../docs/04-seguranca.md)).
    PermitirDesktopProtegido(bool),
    /// Bloqueie a tela desta sessão: o par, que controlava esta máquina, bloqueou a dele.
    BloquearTela,
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
        /// Se entre eles está o seguro, e o agente alcança a tela de bloqueio e a de login.
        ///
        /// Decidido pelo agente, com a regra de `ir_input::desktop`: o serviço não reinterpreta os
        /// nomes.
        tela_de_bloqueio: bool,
    },
    /// O desktop que está recebendo entrada mudou.
    ///
    /// No Windows acontece a cada bloqueio de tela e a cada prompt de UAC. Não existe
    /// notificação do sistema para isto, então o agente descobre consultando
    /// ([00b, §2](../../../docs/00-licoes-do-deskflow.md)).
    DesktopMudou {
        /// O nome do desktop que passou a receber entrada, para o registro.
        nome: String,
        /// Se ele é protegido — tudo que não é a área de trabalho. Decidido pelo agente.
        protegido: bool,
    },
    /// A entrada local: tecla, botão, roda ou ponteiro, como a captura a viu.
    ///
    /// O tipo é o mesmo que a captura do Linux entrega direto ao serviço, e os dois seguem pelo
    /// mesmo caminho até a sessão.
    Capturado(Capture),
    /// O usuário acionou o atalho de emergência.
    Emergencia,
    /// O arranjo de telas mudou.
    TelasMudaram(ir_proto::screens::ScreenLayout),
    /// O agente está saindo, ou a conexão com ele caiu.
    ///
    /// O serviço precisa saber para parar de contar com ele: enquanto não houver agente, não há
    /// quem injete nem quem capture nesta máquina, e insistir em mandar comandos para o vazio
    /// deixaria a interface dizendo que está tudo bem quando não está.
    Encerrou,
    /// A injeção foi recusada pelo sistema.
    ///
    /// É o sintoma do endurecimento de janeiro de 2026 quando as três origens confiáveis não
    /// são atendidas ([05, §4.4](../../../docs/05-windows.md)). O agente **precisa** contar,
    /// porque do ponto de vista do usuário nada aconteceu e ele não teria como saber.
    InjecaoRecusada {
        /// Em qual desktop.
        desktop: String,
        /// Se esse desktop é protegido: aí a recusa é a política, e não o sistema.
        protegido: bool,
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
                protegido: true,
            },
            FatoDoAgente::InjecaoRecusada {
                desktop: "Winlogon".to_owned(),
                protegido: true,
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
    fn a_entrada_atravessa_o_canal_do_agente_sem_traducao() {
        use ir_proto::ids::MonitorId;
        let comando = ComandoDoAgente::Injetar(Injection::Pointer(PointerPosition {
            monitor: MonitorId(1),
            x: 7,
            y: 9,
        }));
        let fato = FatoDoAgente::Capturado(Capture::PointerMotion { dx: -3, dy: 4 });
        let mut canal = Vec::new();
        crate::codec::escrever_em(&mut canal, &comando).expect("codifica");
        crate::codec::escrever_em(&mut canal, &fato).expect("codifica");
        let mut leitura = canal.as_slice();
        let lido: Option<ComandoDoAgente> = crate::codec::ler_de(&mut leitura).expect("decodifica");
        assert_eq!(lido, Some(comando));
        let lido: Option<FatoDoAgente> = crate::codec::ler_de(&mut leitura).expect("decodifica");
        assert_eq!(lido, Some(fato));
    }

    #[test]
    fn a_recusa_de_injecao_diz_em_qual_desktop() {
        // Sem o desktop, o diagnóstico não distingue "recusou na tela de bloqueio" de
        // "recusou no desktop normal" — que são problemas completamente diferentes.
        let fato = FatoDoAgente::InjecaoRecusada {
            desktop: "Winlogon".to_owned(),
            protegido: true,
        };
        match fato {
            FatoDoAgente::InjecaoRecusada { desktop, .. } => assert_eq!(desktop, "Winlogon"),
            _ => panic!("variante errada"),
        }
    }
}
