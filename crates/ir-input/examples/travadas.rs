//! Bancada da tecla presa: prova, contra o Windows de verdade, que devolver o controle solta os
//! modificadores que ficaram grudados.
//!
//! ```text
//! cargo run -p ir-input --example travadas
//! ```
//!
//! O defeito: segurar o Ctrl aqui, atravessar para o outro computador e soltá-lo **lá** deixava o
//! Ctrl preso aqui — a supressão come o "soltar", e o Windows segue achando que a tecla está
//! apertada; clicar passa a selecionar vários itens. Aqui o estado preso é criado de propósito
//! (um `Ctrl` apertado e nunca solto) e então o controle é devolvido, que é o momento em que o
//! produto limpa o que a supressão engoliu.
//!
//! Se a correção sumir, esta bancada acusa — e deixa o teclado como encontrou de qualquer jeito.

#![allow(clippy::print_stdout, clippy::expect_used, unsafe_code)]

/// A bancada só existe no Windows: é o estado de tecla **do Windows** que ela mede.
#[cfg(windows)]
mod bancada {
    use std::sync::mpsc;

    use windows::Win32::UI::Input::KeyboardAndMouse::{
        GetAsyncKeyState, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBD_EVENT_FLAGS, KEYBDINPUT,
        KEYEVENTF_KEYUP, SendInput, VIRTUAL_KEY,
    };

    /// O Ctrl esquerdo, pelo código virtual.
    const CTRL: u16 = 0xA2;

    pub(crate) fn main() {
        let (eventos, _recebe) = mpsc::channel();
        let capturador = ir_input::start_capture(eventos).expect("abrindo a captura");

        println!("antes de tudo, o Ctrl está apertado? {}", apertada(CTRL));
        tecla(CTRL, true);
        println!("depois de apertar o Ctrl (sem soltar): {}", apertada(CTRL));

        // O controle vai para o outro computador e volta: é na volta que o produto limpa.
        capturador.set_suppress(true);
        capturador.set_suppress(false);
        let ainda_presa = apertada(CTRL);
        println!("depois de devolver o controle: {ainda_presa}");

        // Deixa o teclado como estava, dê no que der.
        tecla(CTRL, false);

        if ainda_presa {
            println!("\nFALHOU: o Ctrl continuou preso — é o defeito de volta.");
            std::process::exit(1);
        }
        println!("\nok: o Ctrl foi solto ao devolver o controle.");
    }

    /// Se o sistema acha que esta tecla está apertada agora.
    fn apertada(vk: u16) -> bool {
        // SAFETY: a função só lê o estado de uma tecla e não tem pré-condição.
        unsafe { GetAsyncKeyState(i32::from(vk)) < 0 }
    }

    /// Aperta ou solta uma tecla pelo código virtual, como o sistema a entende.
    fn tecla(vk: u16, pressionada: bool) {
        let flags = if pressionada {
            KEYBD_EVENT_FLAGS(0)
        } else {
            KEYEVENTF_KEYUP
        };
        let entrada = INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: VIRTUAL_KEY(vk),
                    wScan: 0,
                    dwFlags: flags,
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        };
        // SAFETY: `entrada` é um `INPUT` bem formado e o tamanho declarado é o do tipo.
        let tamanho = i32::try_from(core::mem::size_of::<INPUT>()).unwrap_or(0);
        unsafe { SendInput(&[entrada], tamanho) };
        std::thread::sleep(std::time::Duration::from_millis(120));
    }
}

#[cfg(windows)]
fn main() {
    bancada::main();
}

/// No Linux quem injeta é o `uinput`, e não há estado de tecla do sistema a limpar: o dispositivo
/// virtual é nosso, e soltar o que este injetor apertou já é a garantia (`ir-input/pendentes`).
#[cfg(not(windows))]
fn main() {
    println!("esta bancada é do Windows; no Linux não há modificador preso a soltar");
}
