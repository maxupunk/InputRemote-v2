//! O lançamento do agente na sessão do usuário.
//!
//! É o passo que só o Windows exige, e a razão de o agente existir: um serviço na **sessão 0**
//! não enxerga o teclado nem o mouse do usuário, e o `SendInput` dele não chega ao desktop de
//! ninguém. Quem captura e injeta precisa nascer *dentro* da sessão interativa
//! ([05, §3.1](../../../docs/05-windows.md)).
//!
//! Dois caminhos, conforme quem somos:
//!
//! - **Em primeiro plano** (teste à mão), já estamos na sessão do usuário: basta criar um
//!   processo filho comum.
//! - **Como serviço** (`LocalSystem`, sessão 0): duplica o próprio token, move-o para a sessão de
//!   console, marca `TokenUIAccess` e cria o processo em `WinSta0\Default`. O desktop seguro é
//!   alcançado de dentro, por thread, e não pelo `STARTUPINFO`
//!   ([00b, §4](../../../docs/00-licoes-do-deskflow.md)).
//!
//! O caminho de serviço é o da PoC-1, que já respondeu que ele funciona
//! ([log 07](../../../docs/logs/07-poc1-tela-de-bloqueio.md)).

#![allow(unsafe_code)]

use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::{Context, Result, bail};

/// Se este processo está rodando como serviço do SCM (sessão 0).
///
/// Marcado pelo módulo do serviço antes de qualquer trabalho subir. É mais confiável — e muito
/// mais simples — que deduzir a sessão por API: quem sabe é quem foi lançado.
static COMO_SERVICO: AtomicBool = AtomicBool::new(false);

/// Registra que estamos rodando como serviço.
pub(crate) fn marcar_como_servico() {
    COMO_SERVICO.store(true, Ordering::Relaxed);
}

/// Se estamos rodando como serviço.
pub(crate) fn como_servico() -> bool {
    COMO_SERVICO.load(Ordering::Relaxed)
}

/// O caminho do executável do agente, ao lado do nosso — erro se ele não estiver lá.
fn caminho_do_agente() -> Result<std::path::PathBuf> {
    let exe = std::env::current_exe()
        .context("descobrindo o próprio executável")?
        .with_file_name("inputremote-agent.exe");
    if !exe.exists() {
        bail!("o executável do agente não está em {}", exe.display());
    }
    Ok(exe)
}

/// Lança o ajudante de clipboard na sessão de console, **como o usuário que entrou nela**.
///
/// Não como SYSTEM: o clipboard é dado do usuário, e o ajudante não pode pedir ao serviço nada que o
/// usuário não pudesse pedir pela janela ([ADR-0011](../../../docs/adr/0011-clipboard-na-travessia.md)).
///
/// # Errors
///
/// Sem sessão de console, sem ninguém dentro dela (a tela de login), ou se o sistema recusar.
pub(crate) fn lancar_ajudante_de_clipboard() -> Result<u32> {
    let exe = caminho_do_agente()?;
    janela::lancar_como_usuario(&exe, "--clipboard")
}

/// Lança o agente, pelo caminho que o nosso contexto exigir.
///
/// # Errors
///
/// Erro se o executável do agente não existir, ou se o sistema recusar o lançamento.
pub(crate) fn lancar_agente() -> Result<u32> {
    let exe = caminho_do_agente()?;
    if como_servico() {
        return janela::lancar_na_sessao_de_console(&exe);
    }
    // Já estamos na sessão do usuário: um filho comum basta.
    let filho = std::process::Command::new(&exe)
        .spawn()
        .with_context(|| format!("lançando {}", exe.display()))?;
    Ok(filho.id())
}

#[cfg(windows)]
mod janela {
    use anyhow::{Context, Result, bail};
    use windows::Win32::Foundation::{CloseHandle, HANDLE};
    use windows::Win32::Security::{
        DuplicateTokenEx, SecurityIdentification, SetTokenInformation, TOKEN_ADJUST_DEFAULT,
        TOKEN_ADJUST_SESSIONID, TOKEN_ALL_ACCESS, TOKEN_ASSIGN_PRIMARY, TOKEN_DUPLICATE,
        TOKEN_QUERY, TokenPrimary, TokenSessionId, TokenUIAccess,
    };
    use windows::Win32::System::Environment::{CreateEnvironmentBlock, DestroyEnvironmentBlock};
    use windows::Win32::System::RemoteDesktop::{WTSGetActiveConsoleSessionId, WTSQueryUserToken};
    use windows::Win32::System::Threading::{
        CREATE_NO_WINDOW, CREATE_UNICODE_ENVIRONMENT, CreateProcessAsUserW, GetCurrentProcess,
        OpenProcessToken, PROCESS_INFORMATION, STARTUPINFOW,
    };
    use windows::core::{HSTRING, PWSTR};

    /// O desktop em que o agente nasce.
    ///
    /// Sempre `Default`, nunca `Winlogon`: com thread por desktop, um lançamento basta, e é o
    /// que o Deskflow faz há vinte anos ([ADR-0008](../../../docs/adr/0008-agente-com-thread-por-desktop.md)).
    const DESKTOP: &str = "WinSta0\\Default";

    /// Lança o agente na sessão de console, como SYSTEM com `TokenUIAccess`.
    pub(super) fn lancar_na_sessao_de_console(exe: &std::path::Path) -> Result<u32> {
        let sessao = sessao_de_console().context("nenhuma sessão de console ainda")?;
        let token = token_duplicado().context("duplicando o token do serviço")?;

        let resultado = preparar_e_criar(token, sessao, exe);
        // SAFETY: o token já foi usado; fechá-lo é correto em qualquer resultado.
        unsafe { CloseHandle(token) }.ok();
        resultado
    }

    /// Lança `exe argumentos` na sessão de console, com o token e o ambiente de quem entrou nela.
    pub(super) fn lancar_como_usuario(exe: &std::path::Path, argumentos: &str) -> Result<u32> {
        let sessao = sessao_de_console().context("nenhuma sessão de console ainda")?;
        let mut token = HANDLE::default();
        // SAFETY: `token` é um destino válido. Exige `SeTcbPrivilege`, que `LocalSystem` tem; sem
        // ninguém na sessão, a chamada falha, e é esse o erro que volta.
        unsafe { WTSQueryUserToken(sessao, std::ptr::from_mut(&mut token)) }
            .context("ninguém entrou na sessão de console")?;

        // O ambiente do usuário (`APPDATA`, `TEMP`…), e não o do SYSTEM, que o processo herdaria.
        let mut ambiente: *mut std::ffi::c_void = std::ptr::null_mut();
        // SAFETY: `ambiente` é um destino válido e `token` é o token primário que acabou de vir.
        let com_ambiente = unsafe {
            CreateEnvironmentBlock(std::ptr::from_mut(&mut ambiente), Some(token), false)
        }
        .is_ok();
        let linha = format!("\"{}\" {argumentos}", exe.display());
        let resultado =
            criar_processo(token, &linha, com_ambiente.then_some(ambiente.cast_const()));
        if com_ambiente {
            // SAFETY: o bloco veio de `CreateEnvironmentBlock` e não é mais usado.
            unsafe { DestroyEnvironmentBlock(ambiente) }.ok();
        }
        // SAFETY: o token já foi usado; fechá-lo é correto em qualquer resultado.
        unsafe { CloseHandle(token) }.ok();
        resultado
    }

    /// Move o token para a sessão, marca `UIAccess` e cria o processo.
    fn preparar_e_criar(token: HANDLE, sessao: u32, exe: &std::path::Path) -> Result<u32> {
        mover_para_sessao(token, sessao).context("movendo o token de sessão")?;
        // `UIAccess` só é concedido a executável assinado em pasta protegida; a recusa não é
        // fatal — sem ele o agente ainda serve a sessão desbloqueada (N1).
        if let Err(erro) = marcar_ui_access(token) {
            tracing::warn!(%erro, "TokenUIAccess recusado; o agente fica limitado ao N1");
        }
        let Some(texto) = exe.to_str() else {
            bail!("o caminho do agente não é texto válido");
        };
        criar_processo(token, &format!("\"{texto}\""), None)
    }

    /// A sessão de console, ou nada se ainda não houver.
    ///
    /// `0xFFFFFFFF` é o que a API responde nos primeiros instantes do boot. Tratar isso como
    /// sessão válida faria o agente nascer no vazio ([05, §3.6](../../../docs/05-windows.md)).
    fn sessao_de_console() -> Option<u32> {
        // SAFETY: a função não recebe parâmetro e não falha.
        let id = unsafe { WTSGetActiveConsoleSessionId() };
        if id == u32::MAX { None } else { Some(id) }
    }

    /// Duplica o token deste processo (de `SYSTEM`) como token primário.
    fn token_duplicado() -> Result<HANDLE> {
        let mut proprio = HANDLE::default();
        // SAFETY: `GetCurrentProcess` devolve um pseudo-handle sempre válido, e `proprio` é um
        // destino válido para o token.
        unsafe {
            OpenProcessToken(
                GetCurrentProcess(),
                TOKEN_DUPLICATE | TOKEN_QUERY | TOKEN_ASSIGN_PRIMARY,
                std::ptr::from_mut(&mut proprio),
            )
        }?;

        let mut copia = HANDLE::default();
        // SAFETY: `proprio` é um token válido; pedimos um token primário, que é o que
        // `CreateProcessAsUserW` exige.
        let resultado = unsafe {
            DuplicateTokenEx(
                proprio,
                TOKEN_ALL_ACCESS | TOKEN_ADJUST_SESSIONID | TOKEN_ADJUST_DEFAULT,
                None,
                SecurityIdentification,
                TokenPrimary,
                std::ptr::from_mut(&mut copia),
            )
        };
        // SAFETY: `proprio` não é mais necessário, qualquer que seja o resultado acima.
        unsafe { CloseHandle(proprio) }.ok();
        resultado?;
        Ok(copia)
    }

    /// Move o token para a sessão dada. Exige `SeTcbPrivilege`, que `LocalSystem` tem.
    fn mover_para_sessao(token: HANDLE, sessao: u32) -> Result<()> {
        // SAFETY: `sessao` é um `u32` válido, e o tamanho declarado é o dele.
        unsafe {
            SetTokenInformation(
                token,
                TokenSessionId,
                std::ptr::from_ref(&sessao).cast(),
                u32::try_from(size_of::<u32>()).unwrap_or(4),
            )
        }?;
        Ok(())
    }

    /// Marca `TokenUIAccess`, a segunda das três origens confiáveis
    /// ([05, §4.4](../../../docs/05-windows.md)).
    fn marcar_ui_access(token: HANDLE) -> Result<()> {
        let ligado: u32 = 1;
        // SAFETY: `ligado` é um `u32` válido, e o tamanho declarado é o dele.
        unsafe {
            SetTokenInformation(
                token,
                TokenUIAccess,
                std::ptr::from_ref(&ligado).cast(),
                u32::try_from(size_of::<u32>()).unwrap_or(4),
            )
        }?;
        Ok(())
    }

    /// Cria o processo com o token preparado, e o ambiente dado (ou o nosso).
    fn criar_processo(
        token: HANDLE,
        linha: &str,
        ambiente: Option<*const std::ffi::c_void>,
    ) -> Result<u32> {
        // A linha de comando precisa ser gravável e terminada em nulo, e viver até o fim da
        // chamada — por isso um `Vec` próprio, e não um ponteiro para literal.
        let mut linha: Vec<u16> = linha.encode_utf16().chain(std::iter::once(0)).collect();

        let desktop = HSTRING::from(DESKTOP);
        let inicio = STARTUPINFOW {
            cb: u32::try_from(size_of::<STARTUPINFOW>()).unwrap_or(0),
            lpDesktop: PWSTR(desktop.as_ptr().cast_mut()),
            ..Default::default()
        };
        let mut info = PROCESS_INFORMATION::default();

        // SAFETY: `linha` é uma linha de comando terminada em nulo e viva até o fim da chamada;
        // `inicio` e `info` são estruturas válidas com o tamanho declarado.
        unsafe {
            CreateProcessAsUserW(
                Some(token),
                None,
                Some(PWSTR(linha.as_mut_ptr())),
                None,
                None,
                false,
                CREATE_UNICODE_ENVIRONMENT | CREATE_NO_WINDOW,
                ambiente,
                None,
                std::ptr::from_ref(&inicio),
                std::ptr::from_mut(&mut info),
            )
        }?;

        // SAFETY: os dois handles vêm de `CreateProcessAsUserW` e não serão mais usados.
        unsafe {
            CloseHandle(info.hThread).ok();
            CloseHandle(info.hProcess).ok();
        }
        Ok(info.dwProcessId)
    }
}
