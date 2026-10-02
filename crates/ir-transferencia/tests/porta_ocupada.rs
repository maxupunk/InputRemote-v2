//! A porta de arquivos ocupada na subida não fecha o canal até o próximo reinício.
//!
//! Numa atualização o serviço novo sobe antes de o sistema liberar a porta do antigo. Antes, a
//! primeira falha recusava toda cópia até reiniciar o serviço; agora a porta é tentada de novo, e
//! a cópia pedida depois que ela libera passa.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod comum;

use std::time::Duration;

use comum::{Maquina, parear, subir_com};
use ir_ipc::Aviso;
use ir_ipc::transferencia::{Fase, Sentido};
use ir_transferencia::{Leitor, sem_localizador};
use tokio::sync::broadcast;

/// Uma máquina nesta porta: o teste escolhe as portas, porque uma delas começa ocupada.
fn subir(nome: &str, porta: u16) -> Maquina {
    subir_com(&format!("porta-{nome}"), porta, sem_localizador())
}

/// O próximo fim de envio (concluído ou parado), dentro do prazo.
async fn proximo_fim(avisos: &mut broadcast::Receiver<Aviso>, prazo: Duration) -> Option<Fase> {
    let esperar = async {
        loop {
            if let Ok(Aviso::Transferencia(t)) = avisos.recv().await
                && t.sentido == Sentido::Enviando
                && matches!(t.fase, Fase::Concluida { .. } | Fase::Parada(_))
            {
                return t.fase;
            }
        }
    };
    tokio::time::timeout(prazo, esperar).await.ok()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_porta_liberada_depois_volta_a_copiar_sem_reiniciar() {
    // A porta de A, ocupada por "outro serviço" — em todas as interfaces, como a de verdade.
    let ocupante = std::net::TcpListener::bind("0.0.0.0:0").unwrap();
    let porta_de_a = ocupante.local_addr().unwrap().port();
    let livre = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let porta_de_b = livre.local_addr().unwrap().port();
    drop(livre);

    let mut a = subir("ocupada-a", porta_de_a);
    let b = subir("ocupada-b", porta_de_b);
    parear(&a, &b);
    parear(&b, &a);
    let arquivo = a.pasta.join("bilhete.txt");
    std::fs::write(&arquivo, b"chegou").unwrap();

    // Com a porta ocupada, o pedido é recusado com motivo — e não fica esperando para sempre.
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(a.pedidos.enviar(vec![arquivo.clone()], Leitor::Proprio));
    let fase = proximo_fim(&mut a.avisos, Duration::from_secs(5)).await;
    assert!(matches!(fase, Some(Fase::Parada(_))), "{fase:?}");

    // O antigo solta a porta; na próxima tentativa o canal abre, e a cópia passa.
    drop(ocupante);
    tokio::time::sleep(Duration::from_secs(7)).await;
    assert!(a.pedidos.enviar(vec![arquivo], Leitor::Proprio));
    let fase = proximo_fim(&mut a.avisos, Duration::from_secs(20)).await;
    assert!(
        matches!(fase, Some(Fase::Concluida { .. })),
        "antes, a primeira falha recusava tudo até reiniciar: {fase:?}"
    );

    let _ = std::fs::remove_dir_all(&a.pasta);
    let _ = std::fs::remove_dir_all(&b.pasta);
}
