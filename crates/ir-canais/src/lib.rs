//! O transporte de IPC do serviço: os canais que a interface e o agente usam para falar com ele.
//!
//! Dois canais, com vocabulários que não se misturam ([02, §2](../../../docs/02-arquitetura.md)):
//! o de **controle**, por onde a interface pede e recebe estado ([`ir_ipc::Pedido`] /
//! [`ir_ipc::ParaInterface`]); e o do **agente**, por onde chegam os fatos de captura e saem os
//! comandos de injeção. Aqui está só o de controle; o do agente entra com o agente.
//!
//! O serviço é o dono do estado: nenhuma decisão acontece neste módulo. Ele move quadros entre
//! o socket e o ator, que traduz do estado interno para o [`ir_ipc::Estado`] publicado.

//! Saiu do `ir-daemon` quando o serviço chegou ao teto de tamanho de crate: a fronteira já estava
//! provada — o módulo só conhecia o vocabulário do `ir-ipc`, o portão do `ir-acesso` e o `tokio`.

#![forbid(unsafe_code)]
#![cfg_attr(
    test,
    allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::indexing_slicing,
        clippy::panic
    )
)]

mod agente;
#[cfg(test)]
mod apoio;
mod controle;
mod escuta;
mod pastas;
mod quadros;

use anyhow::{Context, Result};
use ir_ipc::{Aviso, ComandoDoAgente, FatoDoAgente, Resposta};
use tokio::sync::broadcast;
use tokio::sync::mpsc::UnboundedSender;
use tokio::sync::oneshot;

pub use ir_sessao::Ajudantes;
pub use pastas::{Pastas, endereco_das_pastas, iniciar_pastas};

/// Um pedido da interface, com o caminho de volta para a resposta do ator.
#[derive(Debug)]
pub struct PedidoRecebido {
    /// O que a interface pediu.
    pub pedido: ir_ipc::Pedido,
    /// Com a autoridade de quem pediu — o que o serviço pode ler **por** esta pessoa.
    pub leitor: ir_transferencia::Leitor,
    /// Por onde o ator devolve a resposta.
    pub responder: oneshot::Sender<Resposta>,
}

/// Quantos avisos ficam em espera para uma interface antes de os mais velhos serem descartados.
///
/// Generoso: a interface consome rápido, e um pico curto de avisos (uma rodada de descoberta) não
/// deve derrubar ninguém. Uma interface que se atrase além disto se reconcilia pelo próximo
/// estado, então perder avisos antigos é seguro.
const FILA_DE_AVISOS: usize = 256;

/// O nome do canal de controle, sobrescrevível por `IR_CONTROL_ENDPOINT` para o teste.
///
/// Resolvido por [`ir_ipc::endereco`], o mesmo lugar que a interface usa para achar o serviço.
#[must_use]
pub fn endereco_de_controle() -> String {
    ir_ipc::endereco::do_controle()
}

/// Sobe o canal de controle e devolve o emissor de avisos que o ator usa para empurrar mudanças.
///
/// # Errors
///
/// Erro do sistema ao abrir o ponto de escuta.
pub fn iniciar_controle(
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
/// quente do produto. Um agente que se atrase além disto perderia entrada de verdade, então a
/// conexão com ele é derrubada: ele solta tudo ao sair, e o serviço o relança.
const FILA_DE_COMANDOS: usize = 4096;

/// O nome do canal do agente, sobrescrevível por `IR_AGENT_ENDPOINT` para o teste.
///
/// Resolvido por [`ir_ipc::endereco`], o mesmo lugar que o agente usa para achar o serviço.
#[must_use]
pub fn endereco_do_agente() -> String {
    ir_ipc::endereco::do_agente()
}

/// Sobe o canal do agente e devolve o emissor de comandos que o ator usa para mandar injeção.
///
/// # Errors
///
/// Erro do sistema ao abrir o ponto de escuta.
pub fn iniciar_agente(
    fatos: UnboundedSender<FatoDoAgente>,
) -> Result<broadcast::Sender<ComandoDoAgente>> {
    let (comandos, _) = broadcast::channel(FILA_DE_COMANDOS);
    // Restrito de propósito: este canal carrega injeção de entrada, e nenhum processo do usuário
    // pode abri-lo. O agente alcança por rodar como o próprio serviço.
    let escuta = escuta::Escuta::abrir(&endereco_do_agente(), escuta::Acesso::Restrito)?;
    tokio::spawn(agente::servir(escuta, fatos, comandos.clone()));
    Ok(comandos)
}

/// Os dois canais de IPC do serviço, já no ar.
///
/// Dois transportes distintos, de propósito: a interface **não** pode pedir injeção de entrada
/// ([04, §5](../../../docs/04-seguranca.md)), e vocabulários que não se misturam são o que torna
/// essa garantia estrutural em vez de combinada.
#[derive(Debug)]
pub struct Canais {
    /// Por onde o serviço avisa as interfaces conectadas.
    pub avisos: broadcast::Sender<ir_ipc::Aviso>,
    /// Quantos ajudantes de clipboard estão ligados.
    pub ajudantes: Ajudantes,
    /// Por onde o serviço manda comandos ao agente.
    pub agente: broadcast::Sender<ir_ipc::ComandoDoAgente>,
    /// Os pedidos da interface, com o caminho da resposta.
    pub pedidos: tokio::sync::mpsc::UnboundedReceiver<PedidoRecebido>,
    /// Os fatos do agente.
    pub fatos: tokio::sync::mpsc::UnboundedReceiver<ir_ipc::FatoDoAgente>,
}

/// Sobe os dois canais antes do ator, para os emissores já existirem quando ele nascer.
///
/// O canal do agente sobe mesmo no Linux, onde nenhum agente conecta: manter o receptor de
/// fatos vivo é o que impede o laço do ator de girar recebendo `None` sem parar.
///
/// # Errors
///
/// Os de subir cada canal: o endereço ocupado por outra instância do serviço, em geral.
pub fn abrir() -> Result<Canais> {
    let (pedido_tx, pedidos) = tokio::sync::mpsc::unbounded_channel();
    let ajudantes = Ajudantes::default();
    let avisos =
        iniciar_controle(pedido_tx, ajudantes.clone()).context("subindo o canal de controle")?;
    // O ajudante de clipboard roda como o usuário; no Windows, quem garante que ele exista é o
    // serviço (`zelador`). No Linux, o `systemd` do usuário.
    #[cfg(windows)]
    ir_sessao::zelar_pelo_clipboard(ajudantes.clone());
    tracing::info!(endereco = %endereco_de_controle(), "canal de controle no ar");

    let (fato_tx, fatos) = tokio::sync::mpsc::unbounded_channel();
    let agente = iniciar_agente(fato_tx).context("subindo o canal do agente")?;
    tracing::info!(endereco = %endereco_do_agente(), "canal do agente no ar");

    Ok(Canais {
        avisos,
        ajudantes,
        agente,
        pedidos,
        fatos,
    })
}
