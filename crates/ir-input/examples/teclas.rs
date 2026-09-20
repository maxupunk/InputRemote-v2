//! Bancada do teclado: mostra a tecla que o gancho capturou, com o código de produção.
//!
//! ```text
//! cargo run -p ir-input --example teclas -- [segundos]
//! ```
//!
//! Serve para a pergunta "esta tecla atravessa?" sem montar as duas máquinas: se ela não aparecer
//! aqui, ela não sai desta máquina — foi assim que o `PrintScreen` e o teclado numérico foram
//! achados, faltando na tabela de scancodes do Windows. Não suprime nada: o teclado continua
//! funcionando normalmente enquanto isto roda.

#![allow(clippy::print_stdout, clippy::expect_used)]

use std::sync::mpsc;
use std::time::{Duration, Instant};

use ir_input::CaptureEvent;

fn main() {
    let segundos = std::env::args()
        .nth(1)
        .and_then(|texto| texto.parse().ok())
        .unwrap_or(15);
    let (eventos, recebe) = mpsc::channel();
    let _capturador = ir_input::start_capture(eventos).expect("abrindo a captura");

    println!("aperte teclas por {segundos} s (nada é suprimido; só teclas aparecem)");
    let fim = Instant::now() + Duration::from_secs(segundos);
    while let Some(restante) = fim.checked_duration_since(Instant::now()) {
        let Ok(evento) = recebe.recv_timeout(restante) else {
            break;
        };
        // Só teclas, e só o identificador da tecla física: o produto nunca registra conteúdo
        // digitado acima de `trace` ([04, §7](../../../docs/04-seguranca.md)). Aqui é uma
        // ferramenta de bancada, rodada à mão, e é o identificador que se quer ver.
        if let CaptureEvent::Key { usage, pressed } = evento {
            let estado = if pressed { "desce" } else { "sobe " };
            println!("{estado}  HID {:#06x}", usage.get());
        }
    }
    println!("fim");
}
