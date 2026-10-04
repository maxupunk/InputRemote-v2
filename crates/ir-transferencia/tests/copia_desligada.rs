//! Copiar e colar desligado no computador que recebe: a cópia do outro é recusada antes de tocar o
//! disco, e quem copiou fica sabendo por quê. Duas máquinas de verdade, pelo canal de dados.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod comum;

use std::time::Duration;

use comum::{esperar, limpar, parear, subir, terminou};
use ir_ipc::transferencia::{Fase, Motivo, Sentido};
use ir_transferencia::Leitor;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn desligado_aqui_a_copia_de_la_e_recusada_e_nada_chega() {
    let mut a = subir("copia-desligada-a");
    let b = subir("copia-desligada-b");
    parear(&a, &b);
    parear(&b, &a);
    b.pedidos.copias().ligar(false);

    let arquivo = a.pasta.join("relatorio.txt");
    std::fs::write(&arquivo, b"o relatorio").unwrap();
    assert!(a.pedidos.enviar(vec![arquivo], Leitor::Proprio));

    let fim = esperar(
        &mut a.avisos,
        Sentido::Enviando,
        Duration::from_secs(15),
        terminou,
    )
    .await
    .expect("a cópia terminou");
    assert_eq!(fim.fase, Fase::Parada(Motivo::SemPermissao), "{fim:?}");
    let recebidos: Vec<_> = std::fs::read_dir(b.pasta.join("recebidos"))
        .unwrap()
        .collect();
    assert!(recebidos.is_empty(), "nada foi gravado do outro lado");

    // Ligado de novo, a mesma cópia passa.
    b.pedidos.copias().ligar(true);
    let arquivo = a.pasta.join("outro.txt");
    std::fs::write(&arquivo, b"agora vai").unwrap();
    assert!(a.pedidos.enviar(vec![arquivo], Leitor::Proprio));
    let fim = esperar(
        &mut a.avisos,
        Sentido::Enviando,
        Duration::from_secs(15),
        terminou,
    )
    .await
    .expect("a cópia terminou");
    assert!(matches!(fim.fase, Fase::Concluida { .. }), "{fim:?}");
    limpar([a, b]);
}
