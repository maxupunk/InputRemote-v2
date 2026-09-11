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
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::Duration;

use anyhow::{Context, Result};
use windows_service::service::{
    ServiceControl, ServiceControlAccept, ServiceExitCode, ServiceState, ServiceStatus, ServiceType,
};
use windows_service::service_control_handler::{self, ServiceControlHandlerResult};
use windows_service::service_dispatcher;

/// O nome pelo qual o SCM conhece o serviço. Precisa bater com o `Name` do instalador.
const NOME: &str = "InputRemote";

/// O código que o SCM devolve quando um executável comum (não lançado por ele) tenta se
/// registrar. É como sabemos que estamos rodando à mão, e não como serviço.
const NAO_E_SERVICO: i32 = 1063; // ERROR_FAILED_SERVICE_CONTROLLER_CONNECT

/// Tenta rodar como serviço do SCM.
///
/// Devolve `Ok(true)` quando fomos lançados pelo SCM — nesse caso a chamada só retorna depois de
/// o serviço parar. Devolve `Ok(false)` quando não fomos (execução à mão), para o chamador seguir
/// em primeiro plano.
///
/// # Errors
///
/// Qualquer erro do SCM que não seja "não é serviço".
pub(crate) fn tentar_como_servico() -> Result<bool> {
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
    // O estado do serviço é da máquina: sem `IR_DATA_DIR` à mão, fica em `%ProgramData%`, que o
    // SYSTEM sabe escrever e nenhum usuário comum adultera.
    garantir_data_dir();
    // A partir daqui o lançador sabe que está na sessão 0, e que alcançar a sessão do usuário
    // exige o caminho do token em vez de um processo filho comum.
    crate::lancador::marcar_como_servico();

    let (parar_tx, parar_rx) = mpsc::channel();
    let tratador = move |controle| match controle {
        // Parar e desligar são a mesma ação: soltar tudo e encerrar.
        ServiceControl::Stop | ServiceControl::Preshutdown => {
            let _ = parar_tx.send(());
            ServiceControlHandlerResult::NoError
        }
        ServiceControl::Interrogate => ServiceControlHandlerResult::NoError,
        _ => ServiceControlHandlerResult::NotImplemented,
    };
    let status =
        service_control_handler::register(NOME, tratador).context("registrando controles")?;

    let aceitos = ServiceControlAccept::STOP | ServiceControlAccept::PRESHUTDOWN;
    reportar(status, ServiceState::Running, aceitos)?;

    // O serviço roda numa runtime própria, numa thread à parte, para esta poder esperar o sinal
    // de parada do SCM sem bloquear o laço.
    let trabalho = std::thread::spawn(|| {
        let _ = crate::executar_bloqueante();
    });

    // Espera o SCM (ou a queda do próprio trabalho) e então reporta parado.
    while parar_rx.recv_timeout(Duration::from_millis(500)).is_err() {
        if trabalho.is_finished() {
            break;
        }
    }
    reportar(status, ServiceState::Stopped, ServiceControlAccept::empty())?;
    Ok(())
}

/// Reporta um estado ao SCM.
fn reportar(
    status: service_control_handler::ServiceStatusHandle,
    estado: ServiceState,
    aceitos: ServiceControlAccept,
) -> Result<()> {
    status
        .set_service_status(ServiceStatus {
            service_type: ServiceType::OWN_PROCESS,
            current_state: estado,
            controls_accepted: aceitos,
            exit_code: ServiceExitCode::Win32(0),
            checkpoint: 0,
            wait_hint: Duration::default(),
            process_id: None,
        })
        .context("reportando estado ao SCM")?;
    Ok(())
}

/// Aponta `IR_DATA_DIR` para `%ProgramData%\InputRemote` quando ele não veio de fora, e garante
/// que a pasta exista.
fn garantir_data_dir() {
    if std::env::var_os("IR_DATA_DIR").is_some() {
        return;
    }
    let base = std::env::var_os("ProgramData")
        .map_or_else(|| PathBuf::from(r"C:\ProgramData"), PathBuf::from);
    let dir = base.join("InputRemote");
    let _ = std::fs::create_dir_all(&dir);
    // SAFETY: chamado no início do serviço, antes de qualquer thread de trabalho subir; não há
    // leitura concorrente de ambiente.
    unsafe {
        std::env::set_var("IR_DATA_DIR", &dir);
    }
}
