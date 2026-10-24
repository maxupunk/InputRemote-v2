//! Imprime cinco amostras desta máquina, uma por segundo, com os processos que casam com os nomes
//! dados e a lista dos mais pesados — a prova de bancada das fontes.
//!
//! ```text
//! cargo run -p ir-telemetria --example amostra -- docker wsl
//! ```
//!
//! No Windows, rodado como usuário comum, a zona térmica do ACPI costuma vir vazia, e os processos
//! de outros usuários ficam de fora: o serviço lê tudo isso como `SYSTEM`.

use std::sync::mpsc;
use std::time::{Duration, Instant};

use ir_monitor::InteresseEmProcessos;

fn main() -> std::io::Result<()> {
    let (tx, rx) = mpsc::channel();
    let inicio = Instant::now();
    let vigia = ir_telemetria::Vigia::iniciar(move |amostra, processos| {
        tx.send((amostra, processos)).is_ok()
    })?;
    vigia.interessar(InteresseEmProcessos {
        nomes: std::env::args().skip(1).collect(),
        lista: true,
    });
    for (amostra, processos) in rx.iter().take(5) {
        println!("{:>5} ms  {amostra:?}", inicio.elapsed().as_millis());
        for uso in processos
            .iter()
            .flat_map(|p| p.vigiados.iter().chain(&p.lista))
        {
            println!("         {uso:?}");
        }
    }
    std::thread::sleep(Duration::from_millis(10));
    Ok(())
}
