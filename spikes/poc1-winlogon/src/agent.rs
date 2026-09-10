//! O agente da PoC-1: uma thread por desktop, e a vigilância de qual está recebendo entrada.
//!
//! Este é o desenho de [ADR-0008](../../../docs/adr/0008-agente-com-thread-por-desktop.md) em
//! miniatura. A pergunta que ele responde: uma thread amarrada ao desktop `Winlogon`, num
//! processo `SYSTEM` com `TokenUIAccess`, consegue fazer `SendInput` chegar ao campo de senha?
//!
//! O que **não** está aqui, de propósito: rede, protocolo, estado de sessão, tratamento de
//! erro decente. Isso é o produto, e o produto só começa depois de esta pergunta ter resposta.

#![allow(unsafe_code)]

#[path = "log.rs"]
mod log;

use std::sync::atomic::{AtomicU8, Ordering};
use std::time::{Duration, Instant};

use windows::Win32::Foundation::HANDLE;
use windows::Win32::System::StationsAndDesktops::{
    CloseDesktop, DESKTOP_READOBJECTS, GetUserObjectInformationW, HDESK, OpenDesktopW,
    OpenInputDesktop, SetThreadDesktop, UOI_NAME,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    INPUT, INPUT_0, INPUT_KEYBOARD, KEYBD_EVENT_FLAGS, KEYBDINPUT, KEYEVENTF_KEYUP,
    KEYEVENTF_SCANCODE, SendInput, VIRTUAL_KEY,
};
use windows::core::{HSTRING, PCWSTR};

/// Os desktops que interessam.
///
/// `Screen-saver` existe e também recebe entrada, mas não faz parte da pergunta: ninguém
/// digita senha num protetor de tela.
const DESKTOPS: [&str; 2] = ["Default", "Winlogon"];

/// Quanto esperar depois de o desktop seguro assumir, antes de digitar.
///
/// Tempo para quem está testando tirar a mão do teclado e olhar a tela. Curto demais e o
/// resultado se confunde com a digitação da própria pessoa.
const DELAY_BEFORE_TYPING: Duration = Duration::from_secs(20);

/// De quanto em quanto tempo se consulta qual desktop está recebendo entrada.
///
/// 200 ms é o mesmo valor a que o Deskflow chegou, e por raciocínio independente
/// (`docs/00-licoes-do-deskflow.md` §2). Não existe notificação para isto.
const WATCH_INTERVAL: Duration = Duration::from_millis(200);

/// Qual desktop está recebendo entrada, publicado pela thread de vigilância.
///
/// Índice em [`DESKTOPS`], ou [`UNKNOWN`] para qualquer outro.
static INPUT_DESKTOP: AtomicU8 = AtomicU8::new(UNKNOWN);
const UNKNOWN: u8 = u8::MAX;

/// O modo em que o serviço nos lançou, para constar no registro.
fn mode() -> String {
    std::env::var("POC1_MODE").unwrap_or_else(|_| "system-uiaccess".to_owned())
}

fn main() {
    log::header("agente");
    log::line(&format!("integridade do token: {}", token_integrity()));

    std::thread::spawn(watch_input_desktop);

    let mut handles = Vec::new();
    for (index, name) in DESKTOPS.iter().enumerate() {
        let name = (*name).to_owned();
        handles.push(std::thread::spawn(move || desktop_thread(index as u8, &name)));
    }

    for handle in handles {
        let _ = handle.join();
    }
}

/// A thread de um desktop. Amarra-se a ele e espera a vez de digitar.
///
/// `SetThreadDesktop` é a **primeira** coisa que ela faz. Depois de a thread criar janela ou
/// gancho, a chamada é recusada — e o sintoma é uma thread que roda, não dá erro, e injeta no
/// desktop errado. Falha silenciosa é a pior categoria.
fn desktop_thread(index: u8, name: &str) {
    let Some(desktop) = open_desktop(name) else {
        log::line(&format!("{name}: não consegui abrir o desktop"));
        return;
    };

    // SAFETY: `desktop` é um handle válido devolvido por `OpenDesktopW`, e esta thread ainda
    // não criou janela nem gancho — a condição que a documentação exige.
    let attached = unsafe { SetThreadDesktop(desktop) }.is_ok();
    if !attached {
        log::line(&format!("{name}: SetThreadDesktop recusou"));
        // SAFETY: handle válido, e não vai mais ser usado.
        unsafe { CloseDesktop(desktop) }.ok();
        return;
    }
    log::line(&format!("{name}: thread amarrada"));

    let mut became_current: Option<Instant> = None;
    let mut typed = false;

    loop {
        std::thread::sleep(WATCH_INTERVAL);
        let current = INPUT_DESKTOP.load(Ordering::Relaxed) == index;

        if !current {
            became_current = None;
            continue;
        }

        let since = *became_current.get_or_insert_with(Instant::now);
        if !typed && since.elapsed() >= DELAY_BEFORE_TYPING {
            log::line(&format!("{name}: digitando a sequência de teste"));
            let sent = type_sequence();
            log::line(&format!("{name}: SendInput aceitou {sent} eventos"));
            typed = true;
        }
    }
}

/// Vigia qual desktop está recebendo entrada e publica o resultado.
///
/// Um serviço na sessão 0 **não** consegue fazer esta consulta: `OpenInputDesktop` responde
/// sobre a sessão de quem chama. Por isso quem vigia é o agente, que já está do lado certo.
fn watch_input_desktop() {
    let mut last = String::new();
    let mut changed_at = Instant::now();

    loop {
        let name = current_input_desktop().unwrap_or_else(|| "?".to_owned());

        if name != last {
            let index = DESKTOPS
                .iter()
                .position(|candidate| candidate.eq_ignore_ascii_case(&name))
                .map_or(UNKNOWN, |position| position as u8);
            INPUT_DESKTOP.store(index, Ordering::Relaxed);

            let delay = changed_at.elapsed().as_millis();
            log::line(&format!(
                "desktop de entrada: {last:?} -> {name:?} (detectado {delay} ms após a consulta \
                 anterior; o alvo é < 300 ms)"
            ));
            last = name;
            changed_at = Instant::now();
        }

        std::thread::sleep(WATCH_INTERVAL);
    }
}

/// O nome do desktop que está recebendo entrada agora.
fn current_input_desktop() -> Option<String> {
    // SAFETY: chamada sem sinalizador e sem herança, pedindo só leitura. O handle devolvido é
    // fechado antes de sair.
    let desktop = unsafe { OpenInputDesktop(Default::default(), false, DESKTOP_READOBJECTS) }.ok()?;
    let name = desktop_name(HANDLE(desktop.0));
    // SAFETY: handle válido, e não vai mais ser usado.
    unsafe { CloseDesktop(desktop) }.ok();
    name
}

/// O nome de um objeto de desktop.
fn desktop_name(handle: HANDLE) -> Option<String> {
    let mut buffer = [0u16; 256];
    let mut needed = 0u32;

    // SAFETY: `buffer` tem o tamanho que estamos declarando, e `needed` é um `u32` válido.
    let ok = unsafe {
        GetUserObjectInformationW(
            handle,
            UOI_NAME,
            Some(buffer.as_mut_ptr().cast()),
            u32::try_from(std::mem::size_of_val(&buffer)).unwrap_or(0),
            Some(&mut needed),
        )
    };
    ok.ok()?;

    let end = buffer.iter().position(|unit| *unit == 0).unwrap_or(buffer.len());
    Some(String::from_utf16_lossy(buffer.get(..end)?))
}

/// Abre um desktop pelo nome, com acesso total.
fn open_desktop(name: &str) -> Option<HDESK> {
    let wide = HSTRING::from(name);
    // SAFETY: `wide` vive até o fim da chamada, e o acesso pedido é o que a documentação
    // exige para `SetThreadDesktop` mais `SendInput`.
    unsafe { OpenDesktopW(PCWSTR(wide.as_ptr()), Default::default(), false, 0x0000_01FF) }.ok()
}

/// Digita a sequência de teste por scancode.
///
/// Scancode e não código virtual: é o que faz o layout da **máquina controlada** decidir o
/// caractere, que é o requisito da tela de login (`docs/05-windows.md` §4.1).
///
/// A sequência é `poc1` seguido de Enter. Dígitos e letras sem acento, para não depender de
/// layout na hora de conferir o que apareceu na tela.
fn type_sequence() -> u32 {
    // Scancodes do conjunto 1: p, o, c, 1, Enter.
    const SCANCODES: [u16; 5] = [0x19, 0x18, 0x2E, 0x02, 0x1C];

    let mut events = Vec::with_capacity(SCANCODES.len() * 2);
    for scancode in SCANCODES {
        events.push(key_event(scancode, false));
        events.push(key_event(scancode, true));
    }

    // SAFETY: `events` é uma fatia de `INPUT` bem formados, e o tamanho declarado é o do tipo.
    unsafe { SendInput(&events, i32::try_from(std::mem::size_of::<INPUT>()).unwrap_or(0)) }
}

fn key_event(scancode: u16, up: bool) -> INPUT {
    let mut flags = KEYEVENTF_SCANCODE;
    if up {
        flags |= KEYEVENTF_KEYUP;
    }
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: VIRTUAL_KEY(0),
                wScan: scancode,
                dwFlags: KEYBD_EVENT_FLAGS(flags.0),
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

/// A integridade do token deste processo, em texto.
///
/// É o dado que decide a matriz de origem confiável: a Microsoft diz que integridade elevada é
/// uma das três origens aceitas, e `SYSTEM` é *System*, acima de *High*. Registrar isto no log
/// permite correlacionar o resultado com a configuração de fato, e não com a pretendida.
fn token_integrity() -> String {
    use std::process::Command;

    Command::new("whoami")
        .arg("/groups")
        .output()
        .ok()
        .map(|output| String::from_utf8_lossy(&output.stdout).into_owned())
        .and_then(|text| {
            for level in ["Mandatory Label\\System", "Mandatory Label\\High", "Mandatory Label\\Medium"]
            {
                if text.contains(level) {
                    return Some(level.to_owned());
                }
            }
            None
        })
        .unwrap_or_else(|| "não determinada".to_owned())
}

