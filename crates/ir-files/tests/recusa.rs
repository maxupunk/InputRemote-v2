//! O que o destino recusa, e por quê.
//!
//! Recusar é **resposta legítima a pedido legítimo**: não há cota, não há permissão, não há disco,
//! o caminho não é seguro. O par recebe um `Reject` com motivo, entende, e o enlace continua de pé.
//!
//! O que todas estas recusas têm em comum, e é o que cada teste cobra: **nenhuma delas toca o
//! disco**. A pasta de recebidos fica exatamente como estava.
//!
//! A outra metade — o par que diz o que não pode ser verdade — está em `violacao.rs`.

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
    Fim, arvore_de_exemplo, atravessar, conteudo_variado, escrever, ler_arvore,
    tem_montagem_parcial, temp,
};
use ir_files::error::FileError;
use ir_files::{Abertura, Cota, Recepcao};
use ir_proto::message::{BulkMessage, ManifestItem, RejectReason, TransferId};

/// Um manifesto montado à mão, para os casos que `manifesto::montar` nunca produziria.
fn item(caminho: &str, tamanho: u64, pasta: bool) -> ManifestItem {
    ManifestItem {
        path: caminho.to_owned(),
        size: tamanho,
        is_dir: pasta,
    }
}

/// Abre uma recepção com um manifesto arbitrário.
async fn abrir(
    recebidos: &std::path::Path,
    itens: Vec<ManifestItem>,
    total: u64,
    cota: Cota,
) -> Result<Abertura, FileError> {
    Recepcao::abrir(recebidos, (TransferId(1), itens, total), cota, None).await
}

#[tokio::test]
async fn um_caminho_de_fuga_no_manifesto_e_recusado_sem_tocar_o_disco() {
    let temp = temp("recusa-fuga");
    let recebidos = temp.sub("recebidos");

    for fuga in [
        "../fora.txt",
        "a/../../fora.txt",
        "/etc/passwd",
        "C:/Windows/System32/x.dll",
        "a\\b.txt",
    ] {
        let abertura = abrir(&recebidos, vec![item(fuga, 10, false)], 10, Cota::default())
            .await
            .unwrap();
        match abertura {
            Abertura::Recusada { motivo, resposta } => {
                assert_eq!(motivo, RejectReason::UnsafePath, "{fuga}");
                assert!(matches!(resposta, BulkMessage::Reject { .. }));
            }
            Abertura::Aceita { .. } => panic!("{fuga} deveria ter sido recusado"),
        }
        assert!(
            !tem_montagem_parcial(&recebidos),
            "{fuga}: recusa não pode criar montagem"
        );
    }
    assert!(ler_arvore(&recebidos).is_empty());
}

#[tokio::test]
async fn passar_da_cota_e_recusado_antes_de_um_byte_chegar() {
    let temp = temp("recusa-cota");
    let origem_dir = temp.sub("origem");
    let alvo = origem_dir.join("grande.bin");
    escrever(&alvo, &conteudo_variado(5000)).await;
    let recebidos = temp.sub("recebidos");

    let cota = Cota {
        bytes: 1000,
        ..Cota::default()
    };
    let motivo = atravessar(&[alvo], &recebidos, cota).await.recusa();
    assert_eq!(motivo, RejectReason::OverQuota);
    assert!(!tem_montagem_parcial(&recebidos));
    assert!(ler_arvore(&recebidos).is_empty());
}

#[tokio::test]
async fn um_par_sem_permissao_de_arquivos_e_recusado() {
    // A permissão de arquivos é separada e revogável por par (docs/04 §2). Revogá-la tem de
    // funcionar sem depender de cota, de tamanho nem de nada mais.
    let temp = temp("recusa-permissao");
    let origem = arvore_de_exemplo(&temp.sub("origem")).await;
    let recebidos = temp.sub("recebidos");

    let cota = Cota {
        permitido: false,
        ..Cota::default()
    };
    let motivo = atravessar(&[origem], &recebidos, cota).await.recusa();
    assert_eq!(motivo, RejectReason::NotPermitted);
    assert!(ler_arvore(&recebidos).is_empty());
}

#[tokio::test]
async fn itens_demais_e_recusado_pela_contagem() {
    let temp = temp("recusa-itens");
    let recebidos = temp.sub("recebidos");
    let cota = Cota {
        itens: 2,
        ..Cota::default()
    };
    let itens = vec![
        item("a.txt", 0, false),
        item("b.txt", 0, false),
        item("c.txt", 0, false),
    ];
    match abrir(&recebidos, itens, 0, cota).await.unwrap() {
        Abertura::Recusada { motivo, .. } => assert_eq!(motivo, RejectReason::TooManyItems),
        Abertura::Aceita { .. } => panic!("itens demais tinha de ser recusado"),
    }
}

#[tokio::test]
async fn sem_espaco_em_disco_e_recusado_e_nao_uma_escrita_que_falha_no_meio() {
    let temp = temp("recusa-disco");
    let recebidos = temp.sub("recebidos");
    let abertura = Recepcao::abrir(
        &recebidos,
        (TransferId(1), vec![item("x.bin", 1000, false)], 1000),
        Cota::default(),
        Some(999),
    )
    .await
    .unwrap();
    match abertura {
        Abertura::Recusada { motivo, .. } => assert_eq!(motivo, RejectReason::NoDiskSpace),
        Abertura::Aceita { .. } => panic!("sem disco tinha de ser recusado"),
    }
}
#[tokio::test]
async fn uma_recusa_nunca_deixa_rastro_em_disco() {
    // A propriedade somada: qualquer que seja o motivo da recusa, a pasta de recebidos fica como
    // estava.
    let temp = temp("recusa-sem-rastro");
    let recebidos = temp.sub("recebidos");
    let casos: Vec<(Vec<ManifestItem>, u64, Cota)> = vec![
        (vec![item("../fuga", 1, false)], 1, Cota::default()),
        (
            vec![item("x", 10, false)],
            10,
            Cota {
                bytes: 1,
                ..Cota::default()
            },
        ),
        (
            vec![item("x", 1, false)],
            1,
            Cota {
                permitido: false,
                ..Cota::default()
            },
        ),
    ];
    for (itens, total, cota) in casos {
        let abertura = abrir(&recebidos, itens, total, cota).await.unwrap();
        assert!(matches!(abertura, Abertura::Recusada { .. }));
    }
    assert!(ler_arvore(&recebidos).is_empty());
    assert!(!tem_montagem_parcial(&recebidos));
}

#[tokio::test]
async fn o_condutor_de_teste_nao_esconde_falha() {
    // Um teste sobre os testes: se `Fim::publicado()` aceitasse uma falha, todos os outros testes
    // deste arquivo passariam por acidente.
    let temp = temp("recusa-metateste");
    let recebidos = temp.sub("recebidos");
    let fim = atravessar(
        &[temp.caminho().join("nao-existe")],
        &recebidos,
        Cota::default(),
    )
    .await;
    assert!(matches!(fim, Fim::Falhou(_)), "{fim:?}");
}
