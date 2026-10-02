#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::PathBuf;

use ir_files::Leitor;

use super::*;

/// Um pouco depois do prazo: o bastante para a desistência agendada já ter rodado.
const DEPOIS_DO_PRAZO: Duration = Duration::from_secs(61);

fn fases(recebe: &mut broadcast::Receiver<Aviso>) -> Vec<Fase> {
    let mut fases = Vec::new();
    while let Ok(aviso) = recebe.try_recv() {
        if let Aviso::Transferencia(copia) = aviso {
            fases.push(copia.fase);
        }
    }
    fases
}

fn fila_com(caminho: &str) -> Arc<Fila> {
    let fila = Arc::new(Fila::default());
    let _ = fila.pedir(vec![PathBuf::from(caminho)], Leitor::Proprio);
    fila
}

#[tokio::test(start_paused = true)]
async fn a_copia_que_espera_desiste_se_a_conexao_nao_volta_no_prazo() {
    let fila = fila_com("/casa/jogo");
    let (avisos, mut recebe) = broadcast::channel(16);
    mostrar_espera(&fila, &avisos);
    assert_eq!(fases(&mut recebe), [Fase::AguardandoConexao]);

    tokio::time::sleep(DEPOIS_DO_PRAZO).await;
    assert_eq!(fases(&mut recebe), [Fase::Parada(Motivo::CanalCaiu)]);
    assert!(
        fila.tomar().is_none(),
        "não sai mais tarde, quando ninguém mais espera por ela"
    );
}

#[tokio::test(start_paused = true)]
async fn a_copia_que_recomecou_no_prazo_nao_desiste() {
    let fila = fila_com("/casa/jogo");
    let (avisos, mut recebe) = broadcast::channel(16);
    mostrar_espera(&fila, &avisos);
    fila.tomar().expect("o enlace voltou e ela recomeçou");

    tokio::time::sleep(DEPOIS_DO_PRAZO).await;
    assert_eq!(
        fases(&mut recebe),
        [Fase::AguardandoConexao],
        "nada de falha"
    );
}

#[tokio::test(start_paused = true)]
async fn a_espera_aparece_uma_vez_mesmo_mostrada_de_dois_lugares() {
    // Quem conduz o canal mostra a espera ao ver o enlace cair, e o pedido feito com o enlace caído
    // também: os dois juntos não viram dois avisos nem dois prazos.
    let fila = fila_com("/casa/jogo");
    let (avisos, mut recebe) = broadcast::channel(16);
    mostrar_espera(&fila, &avisos);
    mostrar_espera(&fila, &avisos);
    tokio::time::sleep(DEPOIS_DO_PRAZO).await;
    assert_eq!(
        fases(&mut recebe),
        [Fase::AguardandoConexao, Fase::Parada(Motivo::CanalCaiu)]
    );
}

#[tokio::test]
async fn a_queda_no_meio_devolve_a_copia_sem_falar_em_falha() {
    let fila = fila_com("/casa/jogo");
    let (avisos, mut recebe) = broadcast::channel(16);
    let trabalho = fila.tomar().expect("começou");
    envio_caiu(&fila, &avisos, trabalho, ("jogo", (10, 30)));
    assert!(
        fases(&mut recebe).is_empty(),
        "a espera é mostrada por quem conduz"
    );
    assert!(fila.tomar().is_some(), "vai de novo");
}

#[tokio::test]
async fn a_copia_que_derruba_o_enlace_toda_vez_desiste() {
    let fila = fila_com("/casa/jogo");
    let (avisos, mut recebe) = broadcast::channel(16);
    for _ in 1..MAX_QUEDAS {
        let trabalho = fila.tomar().expect("vai de novo");
        envio_caiu(&fila, &avisos, trabalho, ("jogo", (10, 30)));
    }
    assert!(fases(&mut recebe).is_empty());

    let trabalho = fila.tomar().expect("a última tentativa");
    envio_caiu(&fila, &avisos, trabalho, ("jogo", (10, 30)));
    assert_eq!(fases(&mut recebe), [Fase::Parada(Motivo::CanalCaiu)]);
    assert!(fila.tomar().is_none(), "não tenta para sempre");
}

#[tokio::test]
async fn a_copia_cancelada_enquanto_ia_diz_que_foi_cancelada_e_nao_que_caiu() {
    let fila = fila_com("/casa/jogo");
    let (avisos, mut recebe) = broadcast::channel(16);
    let trabalho = fila.tomar().expect("começou");
    let _ = fila.cancelar_tudo();
    envio_caiu(&fila, &avisos, trabalho, ("jogo", (10, 30)));
    assert_eq!(fases(&mut recebe), [Fase::Parada(Motivo::Cancelada)]);
}

#[tokio::test(start_paused = true)]
async fn quem_recebe_espera_o_par_recomecar_e_desiste_se_ele_nao_recomeca() {
    let recepcoes = Arc::new(Recepcoes::default());
    let (avisos, mut recebe) = broadcast::channel(16);
    recepcoes.caiu(&avisos, "jogo", (10, 30));
    assert_eq!(fases(&mut recebe), [Fase::AguardandoConexao]);
    tokio::time::sleep(DEPOIS_DO_PRAZO).await;
    assert_eq!(fases(&mut recebe), [Fase::Parada(Motivo::CanalCaiu)]);
}

#[tokio::test(start_paused = true)]
async fn quem_recebe_nao_fala_em_falha_quando_o_par_recomeca() {
    let recepcoes = Arc::new(Recepcoes::default());
    let (avisos, mut recebe) = broadcast::channel(16);
    recepcoes.caiu(&avisos, "jogo", (10, 30));
    recepcoes.comecou();
    tokio::time::sleep(DEPOIS_DO_PRAZO).await;
    assert_eq!(fases(&mut recebe), [Fase::AguardandoConexao]);
}
