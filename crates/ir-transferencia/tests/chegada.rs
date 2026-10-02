//! Quem recebe conta o que começou a chegar, e onde — o que deixa colar antes do fim.
//!
//! O ajudante de clipboard põe arquivos virtuais no clipboard com esta lista, e lê cada um da
//! montagem enquanto ele chega e, depois da publicação, de `publicada_em`. Se esses dois caminhos
//! não forem os de verdade, a colagem antecipada não acha nada ([log 56](../../../docs/logs/56-o-ajudante-que-sobreviveu.md)).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod comum;

use std::path::Path;
use std::time::Duration;

use comum::{esperar, limpar, parear, subir, terminou};
use ir_ipc::Aviso;
use ir_ipc::transferencia::{Fase, Sentido};
use ir_transferencia::Leitor;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn quem_recebe_anuncia_a_chegada_com_os_caminhos_de_verdade() {
    let a = subir("chegada-a");
    let mut b = subir("chegada-b");
    parear(&a, &b);
    parear(&b, &a);
    let pasta = a.pasta.join("relatório");
    std::fs::create_dir_all(pasta.join("anexos")).unwrap();
    std::fs::write(pasta.join("a.txt"), b"primeiro").unwrap();
    std::fs::write(pasta.join("anexos").join("b.txt"), b"segundo").unwrap();

    assert!(a.pedidos.enviar(vec![pasta], Leitor::Proprio));
    let chegando = tokio::time::timeout(Duration::from_secs(30), async {
        loop {
            if let Ok(Aviso::ArquivosChegando(chegando)) = b.avisos.recv().await {
                return chegando;
            }
        }
    })
    .await
    .expect("quem recebe anunciou a chegada");
    assert_eq!(chegando.nome, "relatório");
    let caminhos: Vec<&str> = chegando.itens.iter().map(|i| i.caminho.as_str()).collect();
    assert!(caminhos.contains(&"relatório/a.txt"), "{caminhos:?}");
    assert!(caminhos.contains(&"relatório/anexos/b.txt"), "{caminhos:?}");
    assert!(
        Path::new(&chegando.montagem).starts_with(b.pasta.join("recebidos")),
        "a montagem fica na pasta de recebidos: {}",
        chegando.montagem
    );

    let recebido = esperar(
        &mut b.avisos,
        Sentido::Recebendo,
        Duration::from_secs(30),
        terminou,
    )
    .await
    .expect("chegou");
    assert!(
        matches!(recebido.fase, Fase::Concluida { .. }),
        "{recebido:?}"
    );
    let publicada = Path::new(&chegando.publicada_em);
    assert_eq!(
        std::fs::read(publicada.join("relatório/anexos/b.txt")).unwrap(),
        b"segundo",
        "depois de publicada, cada item está em publicada_em/<caminho>"
    );

    limpar([a, b]);
}
