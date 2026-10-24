//! Imprime cinco amostras desta máquina, uma por segundo — a prova de bancada das fontes.
//!
//! ```text
//! cargo run -p ir-telemetria --example amostra
//! ```
//!
//! No Windows, rodado como usuário comum, a zona térmica do ACPI costuma vir vazia: o serviço a lê
//! como `SYSTEM`.

use std::sync::mpsc;
use std::time::{Duration, Instant};

fn main() -> std::io::Result<()> {
    let (tx, rx) = mpsc::channel();
    let inicio = Instant::now();
    let _vigia = ir_telemetria::Vigia::iniciar(move |amostra| tx.send(amostra).is_ok())?;
    for amostra in rx.iter().take(5) {
        println!("{:>5} ms  {amostra:?}", inicio.elapsed().as_millis());
    }
    std::thread::sleep(Duration::from_millis(10));
    Ok(())
}
