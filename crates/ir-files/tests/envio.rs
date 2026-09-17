//! O lado que envia, exercitado pela API pública.
//!
//! Estes testes moram aqui, e não dentro de `envio.rs`, por duas razões que apontam para o mesmo
//! lado: o arquivo passou do limite de 400 linhas de `docs/09-padroes-de-codigo.md` §1, e o que
//! eles exercitam é exatamente a superfície que o `ir-daemon` vai usar. Um teste de integração é o
//! lugar de quem olha de fora.

// Um teste de integração é um crate próprio, então a liberação que `#[cfg(test)]` concede à
// biblioteca não chega até aqui.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

mod comum;

use comum::{escrever, temp};
use ir_files::envio::{resumo_de, tem_conteudo};
use ir_files::error::FileError;
use ir_files::{Envio, manifesto};
use ir_proto::limits;
use ir_proto::message::{BulkMessage, ManifestItem, TransferId};

/// Todas as mensagens do corpo, em ordem.
async fn drenar(envio: &mut Envio) -> Vec<BulkMessage> {
    let mut todas = Vec::new();
    while let Some(msg) = envio.proxima().await.unwrap() {
        todas.push(msg);
    }
    todas
}

#[tokio::test]
async fn um_arquivo_vira_inicio_bloco_e_fim() {
    let temp = temp("envio-um");
    let alvo = temp.caminho().join("nota.txt");
    escrever(&alvo, b"doze bytes..").await;

    let plano = manifesto::montar(TransferId(1), std::slice::from_ref(&alvo))
        .await
        .unwrap();
    let mut envio = Envio::novo(plano);
    let corpo = drenar(&mut envio).await;

    assert_eq!(corpo.len(), 3, "início, um bloco e fim: {corpo:?}");
    assert!(matches!(
        corpo.first(),
        Some(BulkMessage::FileStart { item: 0, .. })
    ));
    match corpo.get(1) {
        Some(BulkMessage::FileBlock { offset, data, .. }) => {
            assert_eq!(*offset, 0);
            assert_eq!(data.as_slice(), b"doze bytes..");
        }
        outro => panic!("esperava um bloco, veio {outro:?}"),
    }
    match corpo.get(2) {
        Some(BulkMessage::FileEnd { hash, .. }) => {
            assert_eq!(*hash, resumo_de(&alvo).await.unwrap());
        }
        outro => panic!("esperava o fim, veio {outro:?}"),
    }
    assert_eq!(envio.enviados(), 12);
}

#[tokio::test]
async fn um_arquivo_vazio_ainda_tem_inicio_e_fim() {
    // Sem bloco nenhum, mas o destino precisa do `FileStart` para criar o arquivo e do
    // `FileEnd` para conferir. Um arquivo de zero byte é conteúdo legítimo.
    let temp = temp("envio-vazio");
    let alvo = temp.caminho().join("vazio.txt");
    escrever(&alvo, b"").await;

    let plano = manifesto::montar(TransferId(2), &[alvo]).await.unwrap();
    let corpo = drenar(&mut Envio::novo(plano)).await;
    assert_eq!(corpo.len(), 2, "{corpo:?}");
}

#[tokio::test]
async fn um_arquivo_maior_que_o_bloco_e_picado_com_deslocamento_crescente() {
    let temp = temp("envio-grande");
    let alvo = temp.caminho().join("grande.bin");
    let tamanho = limits::MAX_FILE_BLOCK * 2 + 7;
    escrever(&alvo, &vec![0xa5; tamanho]).await;

    let plano = manifesto::montar(TransferId(3), &[alvo]).await.unwrap();
    let mut envio = Envio::novo(plano);
    let corpo = drenar(&mut envio).await;

    let blocos: Vec<(u64, usize)> = corpo
        .iter()
        .filter_map(|m| match m {
            BulkMessage::FileBlock { offset, data, .. } => Some((*offset, data.len())),
            _ => None,
        })
        .collect();
    assert_eq!(blocos.len(), 3, "{blocos:?}");
    // Os deslocamentos são contíguos e nenhum bloco passa do teto.
    let mut esperado = 0u64;
    for (offset, tamanho_do_bloco) in &blocos {
        assert_eq!(*offset, esperado);
        assert!(*tamanho_do_bloco <= limits::MAX_FILE_BLOCK);
        esperado += *tamanho_do_bloco as u64;
    }
    assert_eq!(envio.enviados(), tamanho as u64);
    assert_eq!(envio.total(), tamanho as u64);
}

#[tokio::test]
async fn diretorio_nao_gera_mensagem_nenhuma() {
    // Quem recebe cria as pastas a partir do manifesto, que ele tem inteiro.
    let temp = temp("envio-pastas");
    let raiz = temp.caminho().join("p");
    escrever(&raiz.join("a").join("b").join("x.txt"), b"x").await;

    let plano = manifesto::montar(TransferId(4), &[raiz]).await.unwrap();
    let pastas = plano.itens.iter().filter(|i| i.is_dir).count();
    assert_eq!(pastas, 3, "p, p/a, p/a/b");

    let corpo = drenar(&mut Envio::novo(plano)).await;
    assert_eq!(corpo.len(), 3, "um só arquivo: início, bloco, fim");
}

#[tokio::test]
async fn varios_arquivos_saem_um_depois_do_outro_e_nunca_intercalados() {
    // O receptor depende disto: ele mantém um arquivo aberto por vez.
    let temp = temp("envio-ordem");
    let raiz = temp.caminho().join("p");
    escrever(&raiz.join("a.txt"), &[1u8; 10]).await;
    escrever(&raiz.join("b.txt"), &[2u8; 10]).await;
    escrever(&raiz.join("c.txt"), &[3u8; 10]).await;

    let plano = manifesto::montar(TransferId(5), &[raiz]).await.unwrap();
    let corpo = drenar(&mut Envio::novo(plano)).await;

    let mut aberto: Option<u32> = None;
    let mut fechados = Vec::new();
    for msg in &corpo {
        match msg {
            BulkMessage::FileStart { item, .. } => {
                assert!(aberto.is_none(), "abriu {item} com outro ainda aberto");
                aberto = Some(*item);
            }
            BulkMessage::FileBlock { item, .. } => {
                assert_eq!(aberto, Some(*item), "bloco de um item que não está aberto");
            }
            BulkMessage::FileEnd { item, .. } => {
                assert_eq!(aberto, Some(*item));
                aberto = None;
                fechados.push(*item);
            }
            outro => panic!("mensagem inesperada no corpo: {outro:?}"),
        }
    }
    assert!(aberto.is_none(), "sobrou arquivo aberto");
    assert_eq!(fechados.len(), 3);
}

#[tokio::test]
async fn um_arquivo_que_encolhe_no_meio_tem_erro_com_nome_proprio() {
    // Sem este erro, o sintoma seria "resumo divergente" no destino — que aponta para
    // corrupção de transporte quando a causa foi o arquivo mudando.
    let temp = temp("envio-encolhe");
    let alvo = temp.caminho().join("muda.bin");
    escrever(&alvo, &vec![7u8; 4096]).await;

    let plano = manifesto::montar(TransferId(6), std::slice::from_ref(&alvo))
        .await
        .unwrap();
    let mut envio = Envio::novo(plano);
    // O manifesto já diz 4096. Trocamos o arquivo antes de a leitura começar.
    escrever(&alvo, b"agora sou pequeno").await;

    let mut erro = None;
    loop {
        match envio.proxima().await {
            Ok(Some(_)) => {}
            Ok(None) => break,
            Err(e) => {
                erro = Some(e);
                break;
            }
        }
    }
    let erro = erro.expect("tinha de acusar a mudança");
    assert!(matches!(erro, FileError::MudouDurante { .. }), "{erro}");
    assert!(!erro.derruba_o_enlace(), "é falha local, não do par");
}

#[tokio::test]
async fn o_manifesto_do_envio_e_o_do_plano() {
    let temp = temp("envio-manifesto");
    let alvo = temp.caminho().join("x.txt");
    escrever(&alvo, b"abc").await;
    let plano = manifesto::montar(TransferId(9), &[alvo]).await.unwrap();
    let envio = Envio::novo(plano.clone());
    match envio.manifesto() {
        BulkMessage::Manifest {
            id,
            items,
            total_bytes,
        } => {
            assert_eq!(id, plano.id);
            assert_eq!(items, plano.itens);
            assert_eq!(total_bytes, 3);
        }
        outro => panic!("{outro:?}"),
    }
}

#[tokio::test]
async fn o_resumo_avulso_bate_com_o_calculado_durante_o_envio() {
    // As duas formas de chegar ao mesmo número: uma segunda leitura, e o cálculo incremental
    // feito enquanto os blocos saem. Se divergirem, o destino recusaria conteúdo correto.
    let temp = temp("envio-resumo");
    let alvo = temp.caminho().join("g.bin");
    let conteudo: Vec<u8> = (0..limits::MAX_FILE_BLOCK * 2 + 3)
        .map(|n| u8::try_from(n % 251).unwrap_or(0))
        .collect();
    escrever(&alvo, &conteudo).await;

    let plano = manifesto::montar(TransferId(7), std::slice::from_ref(&alvo))
        .await
        .unwrap();
    let corpo = drenar(&mut Envio::novo(plano)).await;
    let incremental = corpo.iter().find_map(|m| match m {
        BulkMessage::FileEnd { hash, .. } => Some(*hash),
        _ => None,
    });
    assert_eq!(incremental, Some(resumo_de(&alvo).await.unwrap()));
    assert_eq!(
        incremental,
        Some(*blake3::hash(&conteudo).as_bytes()),
        "e os dois batem com o BLAKE3 do conteúdo"
    );
}

#[test]
fn so_arquivo_tem_conteudo() {
    assert!(tem_conteudo(&ManifestItem {
        path: "a".to_owned(),
        size: 1,
        is_dir: false
    }));
    assert!(!tem_conteudo(&ManifestItem {
        path: "a".to_owned(),
        size: 0,
        is_dir: true
    }));
}
