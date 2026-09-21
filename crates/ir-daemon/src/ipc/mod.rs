//! O transporte de IPC do serviço: os canais que a interface e o agente usam para falar com ele.
//!
//! Dois canais, com vocabulários que não se misturam ([02, §2](../../../docs/02-arquitetura.md)):
//! o de **controle**, por onde a interface pede e recebe estado ([`ir_ipc::Pedido`] /
//! [`ir_ipc::ParaInterface`]); e o do **agente**, por onde chegam os fatos de captura e saem os
//! comandos de injeção. Aqui está só o de controle; o do agente entra com o agente.
//!
//! O serviço é o dono do estado: nenhuma decisão acontece neste módulo. Ele move quadros entre
//! o socket e o ator, que traduz do estado interno para o [`ir_ipc::Estado`] publicado.

pub(crate) mod agente;
pub(crate) mod controle;
mod escuta;
pub(crate) mod quadros;

use anyhow::Result;
use ir_ipc::{Aviso, ComandoDoAgente, FatoDoAgente, Resposta};
use tokio::sync::broadcast;
use tokio::sync::mpsc::UnboundedSender;
use tokio::sync::oneshot;

pub(crate) use ir_sessao::Ajudantes;

/// Um pedido da interface, com o caminho de volta para a resposta do ator.
pub(crate) struct PedidoRecebido {
    /// O que a interface pediu.
    pub(crate) pedido: ir_ipc::Pedido,
    /// Com a autoridade de quem pediu — o que o serviço pode ler **por** esta pessoa.
    pub(crate) leitor: ir_transferencia::Leitor,
    /// Por onde o ator devolve a resposta.
    pub(crate) responder: oneshot::Sender<Resposta>,
}

/// Quantos avisos ficam em espera para uma interface antes de os mais velhos serem descartados.
///
/// Generoso: a interface consome rápido, e um pico curto de avisos (uma rodada de descoberta) não
/// deve derrubar ninguém. Uma interface que se atrase além disto se reconcilia pelo próximo
/// estado, então perder avisos antigos é seguro.
const FILA_DE_AVISOS: usize = 256;

/// O nome do canal de controle, sobrescrevível por `IR_CONTROL_ENDPOINT` para o teste.
///
/// A sobrescrita aceita um caminho completo ou só um nome curto: um valor sem separador vira
/// `\\.\pipe\<nome>` no Windows e um socket em `TMP` no Linux. O nome curto existe porque a
/// barra invertida do caminho de *pipe* não sobrevive a algumas camadas de shell.
#[must_use]
pub(crate) fn endereco_de_controle() -> String {
    match std::env::var("IR_CONTROL_ENDPOINT") {
        Ok(valor) if !valor.is_empty() => expandir_override(&valor),
        _ => padrao(),
    }
}

/// Expande uma sobrescrita curta para um endereço completo da plataforma.
fn expandir_override(valor: &str) -> String {
    let curto = !valor.contains(['\\', '/']);
    #[cfg(windows)]
    {
        if curto {
            format!(r"\\.\pipe\{valor}")
        } else {
            valor.to_owned()
        }
    }
    #[cfg(not(windows))]
    {
        if curto {
            std::env::temp_dir()
                .join(format!("{valor}.sock"))
                .to_string_lossy()
                .into_owned()
        } else {
            valor.to_owned()
        }
    }
}

/// O endereço padrão da plataforma.
fn padrao() -> String {
    #[cfg(windows)]
    {
        r"\\.\pipe\inputremote-control".to_owned()
    }
    #[cfg(not(windows))]
    {
        "/run/inputremote/control.sock".to_owned()
    }
}

/// Sobe o canal de controle e devolve o emissor de avisos que o ator usa para empurrar mudanças.
///
/// # Errors
///
/// Erro do sistema ao abrir o ponto de escuta.
pub(crate) fn iniciar_controle(
    pedidos: UnboundedSender<PedidoRecebido>,
    ajudantes: Ajudantes,
) -> Result<broadcast::Sender<Aviso>> {
    let (avisos, _) = broadcast::channel(FILA_DE_AVISOS);
    // A janela do usuário precisa alcançar este canal: é por ele que ela pergunta o estado e
    // conduz o pareamento. Sem isto, um serviço como SYSTEM tranca a própria interface do lado
    // de fora, e ela cai para o modo de demonstração.
    let escuta = escuta::Escuta::abrir(&endereco_de_controle(), escuta::Acesso::UsuarioInterativo)?;
    tokio::spawn(controle::servir(escuta, pedidos, avisos.clone(), ajudantes));
    Ok(avisos)
}

/// Quantos comandos ficam em espera para o agente antes de os mais velhos serem descartados.
///
/// Maior que a fila de avisos: aqui cada item é uma tecla ou um movimento, e o caminho é o
/// quente do produto. Um agente que se atrase além disto perdeu entrada de verdade, e o aviso
/// de atraso é registrado em nível alto por isso.
const FILA_DE_COMANDOS: usize = 4096;

/// O nome do canal do agente, sobrescrevível por `IR_AGENT_ENDPOINT` para o teste.
#[must_use]
pub(crate) fn endereco_do_agente() -> String {
    match std::env::var("IR_AGENT_ENDPOINT") {
        Ok(valor) if !valor.is_empty() => expandir_override(&valor),
        _ => padrao_do_agente(),
    }
}

/// O endereço padrão do canal do agente.
fn padrao_do_agente() -> String {
    #[cfg(windows)]
    {
        r"\\.\pipe\inputremote-agent".to_owned()
    }
    #[cfg(not(windows))]
    {
        "/run/inputremote/agent.sock".to_owned()
    }
}

/// Sobe o canal do agente e devolve o emissor de comandos que o ator usa para mandar injeção.
///
/// # Errors
///
/// Erro do sistema ao abrir o ponto de escuta.
pub(crate) fn iniciar_agente(
    fatos: UnboundedSender<FatoDoAgente>,
) -> Result<broadcast::Sender<ComandoDoAgente>> {
    let (comandos, _) = broadcast::channel(FILA_DE_COMANDOS);
    // Restrito de propósito: este canal carrega injeção de entrada, e nenhum processo do usuário
    // pode abri-lo. O agente alcança por rodar como o próprio serviço.
    let escuta = escuta::Escuta::abrir(&endereco_do_agente(), escuta::Acesso::Restrito)?;
    tokio::spawn(agente::servir(escuta, fatos, comandos.clone()));
    Ok(comandos)
}
