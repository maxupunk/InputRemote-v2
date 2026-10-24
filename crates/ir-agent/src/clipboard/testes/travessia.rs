//! O que o ajudante manda, e quando: a cópia grande espera a travessia, e a cancelada não volta.

use super::*;

#[test]
fn a_copia_cancelada_nao_volta_na_travessia_seguinte() {
    // A pessoa cancelou a cópia grande: atravessar de novo não pode mandá-la outra vez.
    let mut clip = Mentira {
        dentro: Some(arquivos("/tmp/x")),
        ..Mentira::default()
    };
    let mut eco = Eco::nova();
    let mut saida = Vec::new();
    oferecer(
        &mut saida,
        &mut clip,
        &mut eco,
        &nenhuma(),
        Gatilho::Travessia,
    );
    let cancelada = Transferencia {
        sentido: Sentido::Enviando,
        nome: "x".to_owned(),
        bytes_feitos: 0,
        bytes_total: 1,
        fase: Fase::Parada(ir_ipc::transferencia::Motivo::Cancelada),
    };
    reagir(&cancelada, &mut clip, &mut eco);
    oferecer(
        &mut saida,
        &mut clip,
        &mut eco,
        &nenhuma(),
        Gatilho::Travessia,
    );
    assert_eq!(pedidos(&saida).len(), 1);
}

#[test]
fn a_copia_grande_espera_a_travessia_e_vai_nela() {
    let pasta = std::env::temp_dir().join(format!("ir-ajudante-grande-{}", std::process::id()));
    std::fs::create_dir_all(&pasta).unwrap();
    let video = pasta.join("video.bin");
    std::fs::File::create(&video)
        .unwrap()
        .set_len(grande::LIMITE + 1)
        .unwrap();
    let mut clip = Mentira {
        dentro: Some(Conteudo::Arquivos(vec![video.clone()])),
        ..Mentira::default()
    };
    let mut eco = Eco::nova();
    let mut saida = Vec::new();

    // Copiar só para colar aqui: nada sai, quantas vezes o sistema avisar.
    for _ in 0..3 {
        oferecer(&mut saida, &mut clip, &mut eco, &nenhuma(), Gatilho::Copia);
    }
    assert!(pedidos(&saida).is_empty());

    // O cursor foi para o outro computador: agora vai, uma vez.
    oferecer(
        &mut saida,
        &mut clip,
        &mut eco,
        &nenhuma(),
        Gatilho::Travessia,
    );
    oferecer(
        &mut saida,
        &mut clip,
        &mut eco,
        &nenhuma(),
        Gatilho::Travessia,
    );
    assert_eq!(pedidos(&saida).len(), 1);
    let _ = std::fs::remove_dir_all(&pasta);
}
