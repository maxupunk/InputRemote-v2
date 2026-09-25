//! O laço do Gerenciador de Serviços do Windows (SCM).
//!
//! Um serviço do Windows não é um executável comum: o SCM o lança e espera que ele **se
//! registre** e responda a "iniciar", "parar" e "interrogar" em segundos. Sem esse registro, o
//! `StartService` do instalador estoura o tempo — que é exatamente a falha "Service failed to
//! start" que aparece na instalação. Este módulo faz o registro e traduz os controles do SCM
//! para a parada limpa do serviço.
//!
//! O `windows-service` embrulha a FFI do SCM; a macro que gera o ponto de entrada expande para
//! `unsafe`, então este é o único módulo do serviço que o permite — confinado, como manda
//! [09, §4](../../../docs/09-padroes-de-codigo.md).

#![allow(unsafe_code)]

use std::ffi::OsString;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use tokio::sync::watch;
use windows_service::service::{
    PowerEventParam, ServiceControl, ServiceControlAccept, ServiceExitCode, ServiceState,
    ServiceStatus, ServiceType,
};

use crate::EventoDoSistema;
use windows_service::service_control_handler::{self, ServiceControlHandlerResult};
use windows_service::service_dispatcher;

/// O nome pelo qual o SCM conhece o serviço. Precisa bater com o `Name` do instalador.
const NOME: &str = "InputRemote";

/// O código que o SCM devolve quando um executável comum (não lançado por ele) tenta se
/// registrar. É como sabemos que estamos rodando à mão, e não como serviço.
const NAO_E_SERVICO: i32 = 1063; // ERROR_FAILED_SERVICE_CONTROLLER_CONNECT

/// Quanto o serviço espera a parada limpa antes de se declarar parado mesmo assim.
///
/// Folgado para soltar tudo e dispensar o agente, e curto o bastante para o SCM e o instalador não
/// acharem que o serviço travou.
const PRAZO_DE_PARADA: Duration = Duration::from_secs(5);

/// O trabalho do serviço: roda até `parada` pedir, ouvindo o que o sistema avisa por `sistema`.
pub type Trabalho =
    fn(watch::Receiver<bool>, tokio::sync::mpsc::UnboundedReceiver<EventoDoSistema>) -> Result<()>;

/// O trabalho que o SCM vai rodar. Guardado aqui porque o ponto de entrada que o SCM chama é uma
/// função sem contexto, gerada pela macro do `windows-service`.
static TRABALHO: std::sync::OnceLock<Trabalho> = std::sync::OnceLock::new();

/// Tenta rodar como serviço do SCM.
///
/// Devolve `Ok(true)` quando fomos lançados pelo SCM — nesse caso a chamada só retorna depois de
/// o serviço parar. Devolve `Ok(false)` quando não fomos (execução à mão), para o chamador seguir
/// em primeiro plano.
///
/// # Errors
///
/// Qualquer erro do SCM que não seja "não é serviço".
pub fn tentar_como_servico(trabalho: Trabalho) -> Result<bool> {
    let _ = TRABALHO.set(trabalho);
    match service_dispatcher::start(NOME, ffi_service_main) {
        Ok(()) => Ok(true),
        Err(windows_service::Error::Winapi(erro)) if erro.raw_os_error() == Some(NAO_E_SERVICO) => {
            Ok(false)
        }
        Err(erro) => Err(erro).context("registrando no SCM"),
    }
}

windows_service::define_windows_service!(ffi_service_main, service_main);

/// O ponto de entrada que o SCM chama. Sem console para relatar, um erro aqui vira o serviço não
/// subir — o `ErrorControl` do instalador cuida da mensagem ao usuário.
fn service_main(_argumentos: Vec<OsString>) {
    let _ = rodar_servico();
}

/// Registra o tratador de controles, reporta "rodando", e roda o serviço até o SCM mandar parar.
fn rodar_servico() -> Result<()> {
    // A partir daqui o lançador sabe que está na sessão 0, e que alcançar a sessão do usuário
    // exige o caminho do token em vez de um processo filho comum — e o estado vai para a pasta da
    // máquina (`ir_configuracao::data_dir`).
    ir_sessao::marcar_como_servico();

    let (parar_tx, parar_rx) = mpsc::channel();
    let (sistema_tx, sistema_rx) = tokio::sync::mpsc::unbounded_channel();
    let tratador = move |controle| tratar_controle(controle, &parar_tx, &sistema_tx);
    let status =
        service_control_handler::register(NOME, tratador).context("registrando controles")?;

    let aceitos = ServiceControlAccept::STOP
        | ServiceControlAccept::PRESHUTDOWN
        | ServiceControlAccept::POWER_EVENT
        | ServiceControlAccept::SESSION_CHANGE;
    reportar(status, ServiceState::Running, aceitos, Duration::ZERO, 0)?;

    // O serviço roda numa runtime própria, numa thread à parte, para esta poder esperar o sinal
    // de parada do SCM sem bloquear o laço. O canal de parada é o que deixa o laço sair limpo.
    let (pedir_parada, parada) = watch::channel(false);
    let executar = *TRABALHO.get().context("o serviço subiu sem trabalho")?;
    let trabalho = std::thread::spawn(move || executar(parada, sistema_rx));

    // Espera o SCM (ou a queda do próprio trabalho).
    let mut caiu_sozinho = false;
    while parar_rx.recv_timeout(Duration::from_millis(500)).is_err() {
        if trabalho.is_finished() {
            caiu_sozinho = true;
            break;
        }
    }
    parar(status, &pedir_parada, trabalho, caiu_sozinho)
}

/// Traduz um controle do SCM: parar, ou um aviso do sistema para o ator.
fn tratar_controle(
    controle: ServiceControl,
    parar_tx: &mpsc::Sender<()>,
    sistema_tx: &tokio::sync::mpsc::UnboundedSender<EventoDoSistema>,
) -> ServiceControlHandlerResult {
    match controle {
        // Parar e desligar são a mesma ação: soltar tudo e encerrar.
        ServiceControl::Stop | ServiceControl::Preshutdown => {
            let _ = parar_tx.send(());
        }
        ServiceControl::PowerEvent(PowerEventParam::Suspend) => {
            let _ = sistema_tx.send(EventoDoSistema::Suspendendo);
            // O Windows dá uns dois segundos antes de dormir. Um instante para o ator soltar tudo
            // e o adeus sair pela rede — depois de dormir, ninguém mais solta nada.
            std::thread::sleep(PRAZO_ANTES_DE_DORMIR);
        }
        ServiceControl::PowerEvent(
            PowerEventParam::ResumeAutomatic | PowerEventParam::ResumeSuspend,
        ) => {
            let _ = sistema_tx.send(EventoDoSistema::Retomou);
        }
        ServiceControl::SessionChange(mudanca) => {
            let evento =
                if mudanca.reason == windows_service::service::SessionChangeReason::SessionLock {
                    EventoDoSistema::TelaBloqueada
                } else {
                    EventoDoSistema::SessaoMudou
                };
            let _ = sistema_tx.send(evento);
        }
        ServiceControl::Interrogate | ServiceControl::PowerEvent(_) => {}
        _ => return ServiceControlHandlerResult::NotImplemented,
    }
    ServiceControlHandlerResult::NoError
}

/// Pede a parada, espera o trabalho soltar tudo, e reporta "parado" com o código certo.
///
/// Só se declara parado depois de soltar tudo. Declarar antes deixaria o agente vivo além do
/// "parado" — e é nesse intervalo que um instalador tenta trocar o arquivo dele.
fn parar(
    status: service_control_handler::ServiceStatusHandle,
    pedir_parada: &watch::Sender<bool>,
    trabalho: std::thread::JoinHandle<Result<()>>,
    caiu_sozinho: bool,
) -> Result<()> {
    reportar(
        status,
        ServiceState::StopPending,
        ServiceControlAccept::empty(),
        PRAZO_DE_PARADA,
        0,
    )?;
    let _ = pedir_parada.send(true);
    let limite = Instant::now() + PRAZO_DE_PARADA;
    while !trabalho.is_finished() && Instant::now() < limite {
        std::thread::sleep(Duration::from_millis(50));
    }
    // Um trabalho que terminou sem ninguém pedir é falha, e o SCM precisa ouvir isso: só uma saída
    // com código diferente de zero dispara o reinício automático configurado pelo instalador.
    // Antes a saída era sempre zero, e o serviço que caía ficava parado até alguém notar.
    let codigo = if caiu_sozinho {
        match trabalho.join() {
            Ok(Ok(())) => CODIGO_SAIU_SOZINHO,
            _ => CODIGO_FALHOU,
        }
    } else {
        0
    };
    reportar(
        status,
        ServiceState::Stopped,
        ServiceControlAccept::empty(),
        Duration::ZERO,
        codigo,
    )
}

/// Quanto o controle de suspensão segura o Windows para o ator soltar tudo e avisar o par.
const PRAZO_ANTES_DE_DORMIR: Duration = Duration::from_millis(400);
/// O código de saída de um serviço cujo laço terminou sem pedido de parada.
const CODIGO_SAIU_SOZINHO: u32 = 1;
/// O código de saída de um serviço que não conseguiu subir, ou caiu com erro.
const CODIGO_FALHOU: u32 = 2;

/// Reporta um estado ao SCM, com quanto tempo ele deve esperar pelo próximo e o código de saída —
/// zero é sucesso; outro é específico do serviço, e é o que dispara o reinício automático.
fn reportar(
    status: service_control_handler::ServiceStatusHandle,
    estado: ServiceState,
    aceitos: ServiceControlAccept,
    espera: Duration,
    codigo: u32,
) -> Result<()> {
    status
        .set_service_status(ServiceStatus {
            service_type: ServiceType::OWN_PROCESS,
            current_state: estado,
            controls_accepted: aceitos,
            exit_code: if codigo == 0 {
                ServiceExitCode::Win32(0)
            } else {
                ServiceExitCode::ServiceSpecific(codigo)
            },
            checkpoint: 0,
            wait_hint: espera,
            process_id: None,
        })
        .context("reportando estado ao SCM")?;
    Ok(())
}

/// Fecha a pasta de estado a quem não é o serviço nem administrador, a cada subida.
///
/// A cada subida, e não só na instalação: é o que conserta uma máquina instalada antes desta
/// correção, cuja chave herdou a leitura de todos os usuários ([04, §4](../../../docs/04-seguranca.md)).
/// A pasta de recebidos padrão (`recebidos`) mora dentro dela e é reaberta ao usuário interativo,
/// que é para quem os arquivos chegam. Uma falha fica no registro e não impede a subida: sem o
/// serviço, o usuário perde o teclado, e a chave continua tão exposta quanto antes.
pub fn fechar_pasta_de_estado(dir: &std::path::Path, recebidos: &std::path::Path) {
    use ir_acesso::seguranca::proteger_pasta;
    if let Err(erro) = proteger_pasta(dir, ir_acesso::SDDL_PASTA_DE_ESTADO) {
        tracing::error!(%erro, "não foi possível fechar a pasta de estado");
    }
    let _ = std::fs::create_dir_all(recebidos);
    if let Err(erro) = proteger_pasta(recebidos, ir_acesso::SDDL_PASTA_DE_RECEBIDOS) {
        tracing::error!(%erro, "não foi possível abrir a pasta de recebidos ao usuário");
    }
}
