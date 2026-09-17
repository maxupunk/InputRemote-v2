//! A travessia inteira, de um lado ao outro, sem socket no meio.
//!
//! O que estes testes cobram é a promessa que o usuário pediu em uma frase: *"TCP com garantia de
//! cópia de arquivo"*. Garantia, aqui, tem um significado verificável — a árvore que chega é
//! **byte a byte** a que saiu, ou não chega nada.

// Um teste de integração é um crate próprio, então a liberação que `#[cfg(test)]` concede à
// biblioteca não chega até aqui. Em teste, `unwrap` e `panic` com mensagem são o diagnóstico.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

mod comum;

use comum::{
    Fim, arvore_de_exemplo, atravessar, atravessar_com, conteudo_variado, escrever, ler_arvore,
    tem_montagem_parcial, temp,
};
use ir_files::Cota;
use ir_files::error::FileError;
use ir_proto::message::BulkMessage;

#[tokio::test]
async fn a_arvore_chega_byte_a_byte_igual() {
    let temp = temp("travessia-igual");
    let origem = arvore_de_exemplo(&temp.sub("origem")).await;
    let recebidos = temp.sub("recebidos");

    let publicado = atravessar(std::slice::from_ref(&origem), &recebidos, Cota::default())
        .await
        .publicado();

    let antes = ler_arvore(&origem);
    let depois = ler_arvore(&publicado);
    assert_eq!(
        antes.keys().collect::<Vec<_>>(),
        depois.keys().collect::<Vec<_>>(),
        "a lista de caminhos mudou na travessia"
    );
    assert_eq!(antes, depois, "o conteúdo mudou na travessia");
    assert!(!antes.is_empty(), "a árvore de exemplo não pode ser vazia");
}

#[tokio::test]
async fn a_pasta_publicada_leva_o_nome_do_que_foi_copiado() {
    // O usuário copiou "relatório de janeiro"; é o que ele espera ver do outro lado, e não um
    // número de transferência.
    let temp = temp("travessia-nome");
    let origem = arvore_de_exemplo(&temp.sub("origem")).await;
    let recebidos = temp.sub("recebidos");

    let publicado = atravessar(&[origem], &recebidos, Cota::default())
        .await
        .publicado();
    assert_eq!(
        publicado.file_name().and_then(|n| n.to_str()),
        Some("relatório de janeiro")
    );
}

#[tokio::test]
async fn um_arquivo_maior_que_um_bloco_nao_perde_nem_repete_pedaco() {
    // O caso que o enquadramento existe para resolver, e o mais fácil de errar: o conteúdo é
    // variado de propósito, porque bytes todos iguais esconderiam bloco fora de ordem.
    let temp = temp("travessia-grande");
    let origem_dir = temp.sub("origem");
    let alvo = origem_dir.join("grande.bin");
    let conteudo = conteudo_variado(ir_proto::limits::MAX_FILE_BLOCK * 5 + 777);
    escrever(&alvo, &conteudo).await;
    let recebidos = temp.sub("recebidos");

    let publicado = atravessar(&[alvo], &recebidos, Cota::default())
        .await
        .publicado();
    // Um arquivo só é publicado como arquivo, e não como pasta com um arquivo dentro: colar tem
    // de dar o que foi copiado.
    assert!(publicado.is_file(), "{publicado:?} deveria ser o arquivo");
    assert_eq!(
        publicado.file_name().and_then(|n| n.to_str()),
        Some("grande.bin")
    );
    let chegou = tokio::fs::read(&publicado).await.unwrap();
    assert_eq!(chegou.len(), conteudo.len());
    assert_eq!(chegou, conteudo);
}

#[tokio::test]
async fn varios_arquivos_soltos_chegam_juntos() {
    let temp = temp("travessia-varios");
    let origem_dir = temp.sub("origem");
    let mut origens = Vec::new();
    for n in 0..5u8 {
        let alvo = origem_dir.join(format!("arquivo-{n}.bin"));
        escrever(&alvo, &conteudo_variado(1000 + usize::from(n))).await;
        origens.push(alvo);
    }
    let recebidos = temp.sub("recebidos");

    let publicado = atravessar(&origens, &recebidos, Cota::default())
        .await
        .publicado();
    let chegou = ler_arvore(&publicado);
    assert_eq!(chegou.len(), 5, "{chegou:?}");
    for n in 0..5u8 {
        let nome = format!("arquivo-{n}.bin");
        assert_eq!(
            chegou.get(&nome).map(Vec::len),
            Some(1000 + usize::from(n)),
            "{nome}"
        );
    }
}

#[tokio::test]
async fn um_byte_trocado_no_caminho_e_pego_pelo_resumo() {
    // A garantia, no seu caso mais direto. O Noise já protegeria contra adulteração no fio; o
    // BLAKE3 por item existe para pegar o que o Noise não vê — erro de disco, defeito nosso, bloco
    // escrito na posição errada.
    let temp = temp("travessia-adulterado");
    let origem_dir = temp.sub("origem");
    let alvo = origem_dir.join("documento.bin");
    escrever(&alvo, &conteudo_variado(20_000)).await;
    let recebidos = temp.sub("recebidos");

    let fim = atravessar_com(
        &[alvo],
        &recebidos,
        Cota::default(),
        |_, mensagem| match mensagem {
            BulkMessage::FileBlock {
                id,
                item,
                offset,
                mut data,
            } => {
                if let Some(byte) = data.first_mut() {
                    *byte ^= 0x01;
                }
                Some(BulkMessage::FileBlock {
                    id,
                    item,
                    offset,
                    data,
                })
            }
            outra => Some(outra),
        },
    )
    .await;

    let erro = fim.falha();
    assert!(
        matches!(erro, FileError::ResumoDivergente { item: 0 }),
        "{erro}"
    );
    assert!(
        !tem_montagem_parcial(&recebidos),
        "o resumo falhou e a montagem tinha de ter ido embora"
    );
    assert!(
        ler_arvore(&recebidos).is_empty(),
        "nada pode ter sido publicado: {:?}",
        ler_arvore(&recebidos)
    );
}

#[tokio::test]
async fn cancelar_no_meio_nao_deixa_arquivo_temporario_nem_arvore_parcial() {
    // O critério de saída da Etapa 8, literal. O cancelamento aqui é a forma mais crua possível:
    // a mensagem some no caminho e a transferência nunca completa, então a recepção é descartada
    // sem publicar — que é o que acontece quando o enlace cai no meio de 5 GB.
    let temp = temp("travessia-cancela");
    let origem = arvore_de_exemplo(&temp.sub("origem")).await;
    let recebidos = temp.sub("recebidos");

    let fim = atravessar_com(&[origem], &recebidos, Cota::default(), |ordem, mensagem| {
        // Deixa o começo passar e corta no meio do corpo.
        if ordem > 4 { None } else { Some(mensagem) }
    })
    .await;

    // A conclusão é recusada porque falta item por conferir — publicar meia árvore seria entregar
    // ao usuário algo que não é a cópia que ele pediu.
    let erro = fim.falha();
    assert!(
        erro.derruba_o_enlace() || matches!(erro, FileError::Violacao(_)),
        "{erro}"
    );
    assert!(
        !tem_montagem_parcial(&recebidos),
        "sobrou montagem parcial em {recebidos:?}"
    );
    assert!(
        ler_arvore(&recebidos).is_empty(),
        "sobrou coisa em {:?}",
        ler_arvore(&recebidos)
    );
}

#[tokio::test]
async fn duas_entregas_com_o_mesmo_nome_nao_se_sobrescrevem() {
    // Copiar a mesma pasta duas vezes é o caso comum, e perder a primeira cópia porque a segunda
    // tem o mesmo nome seria destruir dado do usuário.
    let temp = temp("travessia-duas");
    let origem_dir = temp.sub("origem");
    let raiz = origem_dir.join("pasta");
    escrever(&raiz.join("x.txt"), b"primeira").await;
    let recebidos = temp.sub("recebidos");

    let primeira = atravessar(std::slice::from_ref(&raiz), &recebidos, Cota::default())
        .await
        .publicado();
    escrever(&raiz.join("x.txt"), b"segunda!").await;
    let segunda = atravessar(&[raiz], &recebidos, Cota::default())
        .await
        .publicado();

    assert_ne!(primeira, segunda);
    assert_eq!(
        tokio::fs::read(primeira.join("x.txt")).await.unwrap(),
        b"primeira"
    );
    assert_eq!(
        tokio::fs::read(segunda.join("x.txt")).await.unwrap(),
        b"segunda!"
    );
}

#[tokio::test]
async fn uma_arvore_so_de_pastas_atravessa() {
    // Zero byte de conteúdo, e ainda assim tem de chegar: o usuário copiou a estrutura.
    let temp = temp("travessia-pastas");
    let origem_dir = temp.sub("origem");
    let raiz = origem_dir.join("estrutura");
    tokio::fs::create_dir_all(raiz.join("a").join("b").join("c"))
        .await
        .unwrap();
    let recebidos = temp.sub("recebidos");

    let publicado = atravessar(std::slice::from_ref(&raiz), &recebidos, Cota::default())
        .await
        .publicado();
    assert_eq!(ler_arvore(&raiz), ler_arvore(&publicado));
    assert!(publicado.join("a").join("b").join("c").is_dir());
}

#[tokio::test]
async fn nada_do_que_atravessa_sai_da_pasta_de_recebidos() {
    // A invariante que mais importa num processo privilegiado: seja o que for que o par diga, o
    // que aparece em disco está debaixo da pasta de destino.
    let temp = temp("travessia-confinada");
    let origem = arvore_de_exemplo(&temp.sub("origem")).await;
    let recebidos = temp.sub("recebidos");

    let publicado = atravessar(&[origem], &recebidos, Cota::default())
        .await
        .publicado();
    assert!(publicado.starts_with(&recebidos));
    let canonico_destino = recebidos.canonicalize().unwrap();
    for caminho in ler_arvore(&publicado).keys() {
        let inteiro = publicado.join(caminho).canonicalize().unwrap();
        assert!(
            inteiro.starts_with(&canonico_destino),
            "{inteiro:?} escapou de {canonico_destino:?}"
        );
    }
}

#[tokio::test]
async fn o_progresso_conta_os_bytes_de_conteudo_e_nao_os_do_protocolo() {
    let temp = temp("travessia-progresso");
    let origem_dir = temp.sub("origem");
    let alvo = origem_dir.join("medido.bin");
    escrever(&alvo, &conteudo_variado(12_345)).await;
    let recebidos = temp.sub("recebidos");

    // O condutor já confere que enviados == escritos; aqui se confere o número em si.
    match atravessar(&[alvo], &recebidos, Cota::default()).await {
        Fim::Publicado(publicado) => {
            let chegou = tokio::fs::metadata(&publicado).await.unwrap();
            assert_eq!(chegou.len(), 12_345);
        }
        outro => panic!("{outro:?}"),
    }
}
