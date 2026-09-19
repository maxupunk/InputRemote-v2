//! Bancada da descoberta: pergunta à rede local quem tem o InputRemote, com o código de produção.
//!
//! ```text
//! cargo run -p ir-net --example descoberta -- [segundos]
//! ```
//!
//! Roda fora do serviço, como o usuário: separa "a rede não entrega o broadcast" de "o serviço não
//! responde".

#![allow(clippy::print_stdout, clippy::expect_used)]

use std::time::Duration;

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let segundos = std::env::args()
        .nth(1)
        .and_then(|texto| texto.parse().ok())
        .unwrap_or(3);
    println!("perguntando à rede local por {segundos} s…");
    let achados = ir_net::descoberta::procurar(Duration::from_secs(segundos))
        .await
        .expect("abre o socket de busca");
    if achados.is_empty() {
        println!("ninguém respondeu");
    }
    for achado in achados {
        println!("{} ({}) em {}", achado.label, achado.machine, achado.addr);
    }
}
