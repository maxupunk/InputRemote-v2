//! Captura por ganchos de baixo nível: `WH_MOUSE_LL` e `WH_KEYBOARD_LL`.
//!
//! O procedimento do gancho **não faz trabalho**: ele lê o evento, empurra para um canal e
//! retorna. O Windows remove em silêncio um gancho que estoura `LowLevelHooksTimeout` (300 ms),
//! e o sintoma é "de repente parou de capturar" sem erro nenhum
//! ([05, §5.1](../../../docs/05-windows.md)).
//!
//! Eventos com a marca de injetado são ignorados: sem isso, quando as duas máquinas rodam o
//! produto, a injeção de um lado seria recapturada e voltaria — laço infinito.
//!
//! Enquanto o controle está no par (supressão ligada), o ponteiro é preso num ponto fixo por
//! `SetCursorPos` a cada movimento, e o evento real é comido (retorno 1) para o cursor local não
//! se mexer ([05, §5.2](../../../docs/05-windows.md)).

#![allow(unsafe_code)]
#![allow(unreachable_pub)]

use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use std::sync::mpsc::Sender;

use ir_proto::input::{Button, WheelDelta};
use windows::Win32::Foundation::{HINSTANCE, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, DispatchMessageW, GetMessageW, GetSystemMetrics, HC_ACTION, KBDLLHOOKSTRUCT,
    MSG, MSLLHOOKSTRUCT, PostThreadMessageW, SM_CXSCREEN, SM_CYSCREEN, SetCursorPos,
    SetWindowsHookExW, TranslateMessage, UnhookWindowsHookEx, WH_KEYBOARD_LL, WH_MOUSE_LL,
    WM_KEYDOWN, WM_KEYUP, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MBUTTONDOWN, WM_MBUTTONUP, WM_MOUSEMOVE,
    WM_MOUSEWHEEL, WM_QUIT, WM_RBUTTONDOWN, WM_RBUTTONUP, WM_SYSKEYDOWN, WM_SYSKEYUP,
    WM_XBUTTONDOWN, WM_XBUTTONUP,
};

use crate::error::{InputError, Result};
use crate::windows::scancode::scancode_to_hid;
use crate::{CaptureEvent, Capturer};

/// O canal por onde os eventos capturados saem. Estático porque o procedimento do gancho é uma
/// função sem contexto.
static SINK: OnceLock<Sender<CaptureEvent>> = OnceLock::new();
/// Se a entrada local está suprimida (controle no par).
static SUPPRESS: AtomicBool = AtomicBool::new(false);
/// O ponto onde o cursor fica preso enquanto o controle está no par.
static TRAP_X: AtomicI32 = AtomicI32::new(0);
static TRAP_Y: AtomicI32 = AtomicI32::new(0);

/// Marcas de evento injetado, para não recapturar a própria injeção.
/// `HC_ACTION` como `i32`, para comparar com o código do gancho sem conversão que avisa.
fn is_action(code: i32) -> bool {
    code == i32::try_from(HC_ACTION).unwrap_or(0)
}

const LLMHF_INJECTED: u32 = 0x0000_0001;
const LLKHF_EXTENDED: u32 = 0x0000_0001;
const LLKHF_INJECTED: u32 = 0x0000_0010;

/// O capturador por ganchos. A tarefa dos ganchos roda numa thread própria com laço de mensagens.
pub struct HookCapturer {
    thread_id: u32,
    handle: Option<std::thread::JoinHandle<()>>,
}

impl core::fmt::Debug for HookCapturer {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("HookCapturer")
    }
}

impl HookCapturer {
    /// Instala os ganchos e começa a capturar, entregando por `sink`.
    ///
    /// # Errors
    ///
    /// [`InputError::Device`] se a thread de ganchos não confirmar a instalação.
    pub fn start(sink: Sender<CaptureEvent>) -> Result<Self> {
        let _ = SINK.set(sink);
        center_trap();

        let (ready_tx, ready_rx) = std::sync::mpsc::channel::<Option<u32>>();
        let handle = std::thread::Builder::new()
            .name("ir-hooks".to_owned())
            .spawn(move || hook_thread(&ready_tx))
            .map_err(|e| InputError::Device(e.to_string()))?;

        match ready_rx.recv() {
            Ok(Some(thread_id)) => Ok(Self {
                thread_id,
                handle: Some(handle),
            }),
            _ => Err(InputError::Device(
                "os ganchos não foram instalados".to_owned(),
            )),
        }
    }
}

impl Capturer for HookCapturer {
    fn set_suppress(&self, on: bool) {
        SUPPRESS.store(on, Ordering::Relaxed);
        if !on {
            // O controle voltou: o que a supressão engoliu não pode ficar preso aqui.
            crate::windows::sendinput::soltar_modificadores_presos();
        }
        if on {
            // Prende o cursor no ponto de captura para os deltas começarem pequenos.
            let (x, y) = (
                TRAP_X.load(Ordering::Relaxed),
                TRAP_Y.load(Ordering::Relaxed),
            );
            warp(x, y);
        }
    }

    fn warp_pointer(&self, x: i32, y: i32) {
        warp(x, y);
    }
}

impl Drop for HookCapturer {
    fn drop(&mut self) {
        // SAFETY: `thread_id` é o id que a thread de ganchos publicou; postar `WM_QUIT` encerra o
        // laço de mensagens dela, e então os ganchos são removidos lá dentro.
        unsafe {
            let _ = PostThreadMessageW(self.thread_id, WM_QUIT, WPARAM(0), LPARAM(0));
        }
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

/// Move o cursor para uma posição absoluta de tela.
fn warp(x: i32, y: i32) {
    // SAFETY: `SetCursorPos` recebe dois inteiros e não tem pré-condição de memória.
    unsafe {
        let _ = SetCursorPos(x, y);
    }
}

/// Guarda o centro da tela primária como ponto de captura.
fn center_trap() {
    // SAFETY: `GetSystemMetrics` recebe um índice e devolve um inteiro.
    let (w, h) = unsafe { (GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN)) };
    TRAP_X.store(w / 2, Ordering::Relaxed);
    TRAP_Y.store(h / 2, Ordering::Relaxed);
}

/// A thread dona dos ganchos: instala, roda o laço de mensagens, e remove ao sair.
fn hook_thread(ready: &Sender<Option<u32>>) {
    // SAFETY: `GetModuleHandleW(None)` devolve o módulo do processo atual, válido para instalar um
    // gancho de baixo nível; `mouse_proc`/`keyboard_proc` têm a assinatura exigida.
    let installed = unsafe {
        let module = GetModuleHandleW(None).unwrap_or_default();
        let hinstance = HINSTANCE(module.0);
        let mouse = SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_proc), Some(hinstance), 0);
        let keyboard = SetWindowsHookExW(WH_KEYBOARD_LL, Some(keyboard_proc), Some(hinstance), 0);
        mouse.ok().zip(keyboard.ok())
    };

    let Some((mouse, keyboard)) = installed else {
        let _ = ready.send(None);
        return;
    };

    // SAFETY: chamada sem parâmetro; devolve o id desta thread, que o dono usa para postar
    // `WM_QUIT`.
    let thread_id = unsafe { GetCurrentThreadId() };
    let _ = ready.send(Some(thread_id));

    run_message_loop();

    // SAFETY: os dois handles vieram de `SetWindowsHookExW` e não serão mais usados.
    unsafe {
        let _ = UnhookWindowsHookEx(mouse);
        let _ = UnhookWindowsHookEx(keyboard);
    }
}

/// O laço de mensagens da thread de ganchos. `GetMessageW` devolve 0 ao receber `WM_QUIT`.
fn run_message_loop() {
    let mut msg = MSG::default();
    loop {
        // SAFETY: `msg` é um destino válido; sem janela, pegamos as mensagens da thread.
        let result = unsafe { GetMessageW(std::ptr::from_mut(&mut msg), None, 0, 0) };
        if result.0 <= 0 {
            break;
        }
        // SAFETY: `msg` foi preenchida por `GetMessageW`.
        unsafe {
            let _ = TranslateMessage(std::ptr::from_ref(&msg));
            DispatchMessageW(std::ptr::from_ref(&msg));
        }
    }
}

/// Manda um evento capturado adiante, se houver quem receba.
fn emit(event: CaptureEvent) {
    if let Some(sink) = SINK.get() {
        let _ = sink.send(event);
    }
}

/// O procedimento do gancho de mouse.
unsafe extern "system" fn mouse_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if !is_action(code) {
        // SAFETY: repasse padrão de gancho.
        return unsafe { CallNextHookEx(None, code, wparam, lparam) };
    }
    // SAFETY: para `HC_ACTION`, `lparam` aponta para uma `MSLLHOOKSTRUCT` válida.
    let info = unsafe { *(lparam.0 as *const MSLLHOOKSTRUCT) };
    let injected = info.flags & LLMHF_INJECTED != 0;
    let suppress = SUPPRESS.load(Ordering::Relaxed);
    let eat = handle_mouse(
        u32::try_from(wparam.0).unwrap_or(0),
        &info,
        injected,
        suppress,
    );
    if eat {
        LRESULT(1)
    } else {
        // SAFETY: repasse padrão de gancho.
        unsafe { CallNextHookEx(None, code, wparam, lparam) }
    }
}

/// Trata um evento de mouse e diz se ele deve ser comido.
fn handle_mouse(msg: u32, info: &MSLLHOOKSTRUCT, injected: bool, suppress: bool) -> bool {
    match msg {
        WM_MOUSEMOVE => handle_move(info, injected, suppress),
        WM_LBUTTONDOWN => emit_button(Button::Left, true, suppress),
        WM_LBUTTONUP => emit_button(Button::Left, false, suppress),
        WM_RBUTTONDOWN => emit_button(Button::Right, true, suppress),
        WM_RBUTTONUP => emit_button(Button::Right, false, suppress),
        WM_MBUTTONDOWN => emit_button(Button::Middle, true, suppress),
        WM_MBUTTONUP => emit_button(Button::Middle, false, suppress),
        WM_XBUTTONDOWN => emit_button(x_button(info), true, suppress),
        WM_XBUTTONUP => emit_button(x_button(info), false, suppress),
        WM_MOUSEWHEEL => {
            let notches = high_word_signed(info.mouseData);
            emit(CaptureEvent::Wheel(WheelDelta { dx: 0, dy: notches }));
            suppress
        }
        _ => false,
    }
}

/// Trata o movimento do ponteiro, com a lógica de prisão quando suprimindo.
fn handle_move(info: &MSLLHOOKSTRUCT, injected: bool, suppress: bool) -> bool {
    if injected {
        // Movimento nosso (a prisão, ou um warp): não repassa nem come, e não vira evento.
        return false;
    }
    if suppress {
        // Controle no par: manda o delta a partir do ponto de prisão e reprende o cursor, para
        // ele não sair da tela local. O evento é comido (o cursor local não se mexe).
        let tx = TRAP_X.load(Ordering::Relaxed);
        let ty = TRAP_Y.load(Ordering::Relaxed);
        let (dx, dy) = (info.pt.x - tx, info.pt.y - ty);
        if dx != 0 || dy != 0 {
            emit(CaptureEvent::PointerMotion { dx, dy });
            warp(tx, ty);
        }
        return true;
    }
    // Controle local: manda a posição **absoluta** real, para a sessão detectar a travessia no
    // ponto certo em vez de acumular deltas de um ponto de partida arbitrário. O evento passa,
    // então o cursor local se move normalmente.
    emit(CaptureEvent::PointerAbsolute {
        x: info.pt.x,
        y: info.pt.y,
    });
    false
}

fn emit_button(button: Button, pressed: bool, suppress: bool) -> bool {
    emit(CaptureEvent::Button { button, pressed });
    suppress
}

/// Qual botão lateral, pela parte alta de `mouseData`.
fn x_button(info: &MSLLHOOKSTRUCT) -> Button {
    if (info.mouseData >> 16) & 0xFFFF == 1 {
        Button::Back
    } else {
        Button::Forward
    }
}

/// A parte alta de `mouseData` como inteiro com sinal (delta da roda).
#[allow(clippy::cast_possible_truncation)]
fn high_word_signed(data: u32) -> i16 {
    ((data >> 16) & 0xFFFF) as i16
}

/// O procedimento do gancho de teclado.
unsafe extern "system" fn keyboard_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if !is_action(code) {
        // SAFETY: repasse padrão de gancho.
        return unsafe { CallNextHookEx(None, code, wparam, lparam) };
    }
    // SAFETY: para `HC_ACTION`, `lparam` aponta para uma `KBDLLHOOKSTRUCT` válida.
    let info = unsafe { *(lparam.0 as *const KBDLLHOOKSTRUCT) };
    if info.flags.0 & LLKHF_INJECTED != 0 {
        // SAFETY: repasse; ignoramos a própria injeção.
        return unsafe { CallNextHookEx(None, code, wparam, lparam) };
    }
    let msg = u32::try_from(wparam.0).unwrap_or(0);
    let eat = handle_key(msg, &info);
    if eat {
        LRESULT(1)
    } else {
        // SAFETY: repasse padrão de gancho.
        unsafe { CallNextHookEx(None, code, wparam, lparam) }
    }
}

/// Trata um evento de teclado e diz se ele deve ser comido.
fn handle_key(msg: u32, info: &KBDLLHOOKSTRUCT) -> bool {
    let pressed = matches!(msg, WM_KEYDOWN | WM_SYSKEYDOWN);
    let is_key = matches!(msg, WM_KEYDOWN | WM_SYSKEYDOWN | WM_KEYUP | WM_SYSKEYUP);
    if !is_key {
        return false;
    }
    let extended = info.flags.0 & LLKHF_EXTENDED != 0;
    let scancode = u16::try_from(info.scanCode).unwrap_or(0);
    if let Some(usage) = scancode_to_hid(scancode, extended) {
        emit(CaptureEvent::Key { usage, pressed });
    }
    SUPPRESS.load(Ordering::Relaxed)
}
