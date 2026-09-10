//! O serviço da PoC-1: lança o agente na sessão de console, como `SYSTEM` com `TokenUIAccess`.
//!
//! É o fluxo de [docs/05-windows.md](../../../docs/05-windows.md) §3.1, no menor tamanho que
//! ainda responde a pergunta. O que ele faz, em ordem:
//!
//! 1. descobre a sessão de console — que existe **antes** de qualquer login;
//! 2. duplica o próprio token, que é de `SYSTEM`;
//! 3. move o token para aquela sessão (é aqui que `SeTcbPrivilege` é exigido);
//! 4. marca `TokenUIAccess`, para atender também a segunda das três origens confiáveis;
//! 5. cria o processo em `WinSta0\Default` — o desktop seguro é alcançado de dentro, por
//!    thread, e não pelo `STARTUPINFO`.
//!
//! O passo 5 é o que a leitura do Deskflow corrigiu no nosso desenho original
//! (`docs/00-licoes-do-deskflow.md` §4).

#![allow(unsafe_code)]

#[path = "log.rs"]
mod log;

use std::ffi::OsString;
use std::time::Duration;

use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::Security::{
    DuplicateTokenEx, SecurityIdentification, TOKEN_ADJUST_DEFAULT, TOKEN_ADJUST_SESSIONID,
    TOKEN_ALL_ACCESS, TOKEN_ASSIGN_PRIMARY, TOKEN_DUPLICATE, TOKEN_QUERY, TokenPrimary,
    TokenSessionId, TokenUIAccess, SetTokenInformation,
};
use windows::Win32::System::RemoteDesktop::WTSGetActiveConsoleSessionId;
use windows::Win32::System::Threading::{
    CREATE_NO_WINDOW, CREATE_UNICODE_ENVIRONMENT, CreateProcessAsUserW, OpenProcessToken,
    PROCESS_INFORMATION, STARTUPINFOW, GetCurrentProcess,
};
use windows::core::{HSTRING, PWSTR};

/// O desktop em que o agente nasce.
///
/// Sempre `Default`, nunca `Winlogon`: com thread por desktop, um lançamento aqui basta, e é
/// o que o Deskflow faz há vinte anos.
const AGENT_DESKTOP: &str = "WinSta0\\Default";

/// O modo pedido, que decide quais das três origens confiáveis o agente vai ter.
fn mode() -> String {
    std::env::var("POC1_MODE").unwrap_or_else(|_| "system-uiaccess".to_owned())
}

windows_service::define_windows_service!(ffi_service_main, service_main);

fn main() -> Result<(), windows_service::Error> {
    windows_service::service_dispatcher::start("poc1", ffi_service_main)
}

fn service_main(_arguments: Vec<OsString>) {
    log::header("serviço");

    match spawn_agent() {
        Ok(pid) => log::line(&format!("agente lançado, pid {pid}")),
        Err(error) => log::line(&format!("FALHA ao lançar o agente: {error}")),
    }

    if std::env::var("POC1_SAS").is_ok() {
        std::thread::spawn(try_send_sas);
    }

    // O serviço fica vivo para que o agente sobreviva. Numa PoC isto basta; no produto, o
    // serviço vigia o agente e o ressobe.
    loop {
        std::thread::sleep(Duration::from_secs(60));
    }
}

/// Item 6 da PoC: `SendSAS` produz a tela de Ctrl+Alt+Del?
///
/// `SendInput` **não** gera a Sequência de Atenção Segura — é projeto do Windows, não defeito.
/// O caminho é `SendSAS` de `sas.dll`, chamado pelo serviço, e ele exige a política
/// `SoftwareSASGeneration` habilitada (`docs/05-windows.md` §4.3).
///
/// Carregado dinamicamente porque `sas.dll` não faz parte das bibliotecas de importação
/// comuns, e porque a ausência dela é um resultado válido a registrar em vez de um erro de
/// ligação.
fn try_send_sas() {
    use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryA};
    use windows::core::s;

    std::thread::sleep(Duration::from_secs(40));
    log::line("item 6: tentando SendSAS");

    // SAFETY: nome de biblioteca constante e terminado em nulo.
    let Ok(library) = (unsafe { LoadLibraryA(s!("sas.dll")) }) else {
        log::line("item 6: sas.dll não carregou");
        return;
    };

    // SAFETY: `library` é um módulo válido; o nome do símbolo é constante.
    let Some(address) = (unsafe { GetProcAddress(library, s!("SendSAS")) }) else {
        log::line("item 6: SendSAS não encontrado em sas.dll");
        return;
    };

    type SendSas = unsafe extern "system" fn(bool);
    // SAFETY: a assinatura declarada é a documentada para `SendSAS`. Se a política
    // `SoftwareSASGeneration` estiver desabilitada, a chamada simplesmente não tem efeito —
    // não é indefinida.
    let send_sas: SendSas = unsafe { std::mem::transmute(address) };

    // SAFETY: ver acima. `false` significa "não como usuário", que é o modo de serviço.
    unsafe { send_sas(false) };
    log::line("item 6: SendSAS chamado — a tela de Ctrl+Alt+Del apareceu?");
}

/// Lança o agente na sessão de console.
fn spawn_agent() -> Result<u32, String> {
    let session = console_session().ok_or("nenhuma sessão de console ainda")?;
    log::line(&format!("sessão de console: {session}"));

    let token = duplicated_system_token().map_err(|e| format!("duplicando o token: {e}"))?;

    if mode() != "user" {
        set_session(token, session).map_err(|e| format!("movendo o token de sessão: {e}"))?;
    }

    if mode() == "system-uiaccess" {
        match set_ui_access(token) {
            Ok(()) => log::line("TokenUIAccess marcado"),
            // Não é fatal: a PoC precisa registrar a falha e seguir, porque a matriz de origem
            // confiável quer justamente saber o que acontece sem ele.
            Err(error) => log::line(&format!("TokenUIAccess recusado: {error}")),
        }
    } else {
        log::line("TokenUIAccess não pedido neste modo");
    }

    let pid = create_process(token).map_err(|e| format!("criando o processo: {e}"))?;
    // SAFETY: o token já foi usado e não é mais necessário.
    unsafe { CloseHandle(token) }.ok();
    Ok(pid)
}

/// A sessão de console, ou nada se ainda não houver.
///
/// Devolve `None` para `0xFFFFFFFF`, que é o que a API responde nos primeiros instantes do
/// boot. Tratar isso como uma sessão válida é o defeito que faria o agente nascer no vazio —
/// e é a diferença entre alcançar N3 e ficar em N2 (`docs/05-windows.md` §3.6).
fn console_session() -> Option<u32> {
    // SAFETY: a função não recebe parâmetro e não falha.
    let id = unsafe { WTSGetActiveConsoleSessionId() };
    if id == u32::MAX { None } else { Some(id) }
}

/// Duplica o token deste processo, que é de `SYSTEM`, como token primário.
fn duplicated_system_token() -> windows::core::Result<HANDLE> {
    let mut own = HANDLE::default();
    // SAFETY: `GetCurrentProcess` devolve um pseudo-handle sempre válido, e `own` é um destino
    // válido para o token.
    unsafe {
        OpenProcessToken(
            GetCurrentProcess(),
            TOKEN_DUPLICATE | TOKEN_QUERY | TOKEN_ASSIGN_PRIMARY,
            &mut own,
        )
    }?;

    let mut duplicated = HANDLE::default();
    // SAFETY: `own` é um token válido; pedimos um token primário, que é o que
    // `CreateProcessAsUserW` exige.
    let result = unsafe {
        DuplicateTokenEx(
            own,
            TOKEN_ALL_ACCESS | TOKEN_ADJUST_SESSIONID | TOKEN_ADJUST_DEFAULT,
            None,
            SecurityIdentification,
            TokenPrimary,
            &mut duplicated,
        )
    };
    // SAFETY: `own` não é mais necessário, qualquer que seja o resultado acima.
    unsafe { CloseHandle(own) }.ok();
    result?;

    Ok(duplicated)
}

/// Move o token para a sessão dada.
///
/// Exige `SeTcbPrivilege`, que a conta `LocalSystem` tem. É o passo que um serviço rodando como
/// usuário não conseguiria dar.
fn set_session(token: HANDLE, session: u32) -> windows::core::Result<()> {
    // SAFETY: `session` é um `u32` válido, e o tamanho declarado é o dele.
    unsafe {
        SetTokenInformation(
            token,
            TokenSessionId,
            std::ptr::from_ref(&session).cast(),
            u32::try_from(std::mem::size_of::<u32>()).unwrap_or(4),
        )
    }
}

/// Marca `TokenUIAccess`.
///
/// Só tem efeito se o executável estiver assinado e instalado em local gravável apenas por
/// administradores (`docs/05-windows.md` §4.4). Sem isso, a chamada pode até passar e o
/// privilégio não ser concedido — que é uma das coisas que esta PoC vai medir.
fn set_ui_access(token: HANDLE) -> windows::core::Result<()> {
    let enabled: u32 = 1;
    // SAFETY: `enabled` é um `u32` válido, e o tamanho declarado é o dele.
    unsafe {
        SetTokenInformation(
            token,
            TokenUIAccess,
            std::ptr::from_ref(&enabled).cast(),
            u32::try_from(std::mem::size_of::<u32>()).unwrap_or(4),
        )
    }
}

/// Cria o processo do agente com o token dado.
fn create_process(token: HANDLE) -> windows::core::Result<u32> {
    let exe = std::env::current_exe()?
        .with_file_name("poc1-agent.exe")
        .to_string_lossy()
        .into_owned();
    log::line(&format!("executável do agente: {exe}"));

    let mut wide: Vec<u16> = exe.encode_utf16().chain(std::iter::once(0)).collect();

    let desktop = HSTRING::from(AGENT_DESKTOP);
    let startup = STARTUPINFOW {
        cb: u32::try_from(std::mem::size_of::<STARTUPINFOW>()).unwrap_or(0),
        lpDesktop: PWSTR(desktop.as_ptr().cast_mut()),
        ..Default::default()
    };
    let mut info = PROCESS_INFORMATION::default();

    // SAFETY: `wide` é uma linha de comando terminada em nulo e viva até o fim da chamada;
    // `startup` e `info` são estruturas válidas com o tamanho declarado.
    unsafe {
        CreateProcessAsUserW(
            Some(token),
            None,
            Some(PWSTR(wide.as_mut_ptr())),
            None,
            None,
            false,
            CREATE_UNICODE_ENVIRONMENT | CREATE_NO_WINDOW,
            None,
            None,
            &startup,
            &mut info,
        )
    }?;

    // SAFETY: os dois handles vêm de `CreateProcessAsUserW` e não serão mais usados.
    unsafe {
        CloseHandle(info.hThread).ok();
        CloseHandle(info.hProcess).ok();
    }
    Ok(info.dwProcessId)
}
