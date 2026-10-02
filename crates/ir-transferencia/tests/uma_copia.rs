//! Uma cópia de cada vez, entre duas máquinas de verdade.
//!
//! O relato: Ctrl+C três vezes na mesma pasta copiou a pasta três vezes. E o que se quer quando a
//! pessoa copia **outra** coisa no meio é que a anterior pare e dê lugar — sem deixar sobra no
//! destino.
//!
//! Aqui as duas coisas acontecem contra o canal de dados inteiro: manifesto, blocos, conferência e
//! cancelamento pelo protocolo.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod comum;

use std::path::PathBuf;
use std::time::Duration;

use comum::{parear, subir};
use ir_ipc::Aviso;
use ir_ipc::transferencia::{Fase, Motivo, Sentido, Transferencia};
use ir_transferencia::Leitor;
use tokio::sync::broadcast;

/// Um arquivo grande o bastante para a cópia ainda estar em curso no pedido seguinte.
fn arquivo_grande(pasta: &std::path::Path, nome: &str, megabytes: usize) -> PathBuf {
    let caminho = pasta.join(nome);
    std::fs::write(&caminho, vec![b'k'; megabytes * 1024 * 1024]).unwrap();
    caminho
}

/// Recolhe fins de transferência deste sentido, até juntar `quantos` ou o prazo acabar.
async fn fins(
    avisos: &mut broadcast::Receiver<Aviso>,
    sentido: Sentido,
    quantos: usize,
    prazo: Duration,
) -> Vec<Transferencia> {
    let mut vistos = Vec::new();
    let recolher = async {
        loop {
            if let Ok(Aviso::Transferencia(t)) = avisos.recv().await
                && t.sentido == sentido
                && matches!(t.fase, Fase::Concluida { .. } | Fase::Parada(_))
            {
                vistos.push(t);
                if vistos.len() >= quantos {
                    return;
                }
            }
        }
    };
    let _ = tokio::time::timeout(prazo, recolher).await;
    vistos
}

/// O defeito relatado: o mesmo Ctrl+C repetido copiava a mesma coisa de novo, uma cópia inteira
/// atrás da outra.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn pedir_a_mesma_copia_de_novo_nao_copia_duas_vezes() {
    let mut a = subir("uma-copia-repetida-a");
    let b = subir("uma-copia-repetida-b");
    parear(&a, &b);
    parear(&b, &a);

    // Grande o bastante para ainda estar indo quando os pedidos repetidos chegam — eles chegam no
    // mesmo instante —, e pequeno para a primeira terminar com folga dentro do prazo, mesmo num
    // build de depuração: 24 MB levavam os mesmos doze segundos do prazo.
    let arquivo = arquivo_grande(&a.pasta, "grande.bin", 8);
    for _ in 0..3 {
        assert!(
            a.pedidos.enviar(vec![arquivo.clone()], Leitor::Proprio),
            "o pedido repetido é aceito, e não vira outra cópia"
        );
    }

    // Espera por **duas**: se a segunda não vier no prazo, é exatamente o que se quer provar.
    let terminadas = fins(&mut a.avisos, Sentido::Enviando, 2, Duration::from_secs(12)).await;
    let concluidas = terminadas
        .iter()
        .filter(|t| matches!(t.fase, Fase::Concluida { .. }))
        .count();
    assert_eq!(concluidas, 1, "uma cópia só: {terminadas:?}");

    for pasta in [a.pasta, b.pasta] {
        let _ = std::fs::remove_dir_all(pasta);
    }
}

/// Copiar outra coisa no meio: a anterior para, o destino não fica com sobra, e a nova chega.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn copiar_outra_coisa_cancela_a_anterior_e_nao_deixa_sobra() {
    let mut a = subir("troca-a");
    let mut b = subir("troca-b");
    parear(&a, &b);
    parear(&b, &a);

    let grande = arquivo_grande(&a.pasta, "grande.bin", 64);
    let pequeno = a.pasta.join("bilhete.txt");
    std::fs::write(&pequeno, b"o que o usuario quer de verdade").unwrap();

    assert!(a.pedidos.enviar(vec![grande], Leitor::Proprio));
    // Enquanto a grande está indo, o usuário copia outra coisa.
    tokio::time::sleep(Duration::from_millis(150)).await;
    assert!(a.pedidos.enviar(vec![pequeno], Leitor::Proprio));

    let terminadas = fins(&mut a.avisos, Sentido::Enviando, 2, Duration::from_secs(30)).await;
    let cancelada = terminadas
        .iter()
        .any(|t| matches!(t.fase, Fase::Parada(Motivo::Cancelada)));
    let entregue = terminadas
        .iter()
        .any(|t| matches!(t.fase, Fase::Concluida { .. }) && t.nome.starts_with("bilhete"));
    assert!(cancelada, "a cópia grande deu lugar: {terminadas:?}");
    assert!(entregue, "a nova cópia chegou: {terminadas:?}");

    // O que sobrou no destino: só o que o usuário quis por último. O arquivo grande, cancelado no
    // meio, não pode ter virado arquivo — a montagem só se publica inteira.
    let _ = fins(
        &mut b.avisos,
        Sentido::Recebendo,
        2,
        Duration::from_millis(500),
    )
    .await;
    let recebidos: Vec<String> = std::fs::read_dir(b.pasta.join("recebidos"))
        .unwrap()
        .filter_map(|item| Some(item.ok()?.file_name().to_string_lossy().into_owned()))
        .collect();
    assert!(
        !recebidos.iter().any(|nome| nome.starts_with("grande")),
        "sobrou pedaço da cópia cancelada: {recebidos:?}"
    );

    for pasta in [a.pasta, b.pasta] {
        let _ = std::fs::remove_dir_all(pasta);
    }
}
