//! O par que mente, e o enlace que cai por causa disso.
//!
//! Derrubar é diferente de recusar. Aqui o outro lado não está pedindo algo que não pode ter: está
//! dizendo algo que **não pode ser verdade** — um bloco maior que o tamanho que ele mesmo declarou,
//! um deslocamento fora de ordem num transporte que garante ordem, o fim de um arquivo que nunca
//! começou. Não há resposta cortês a dar, porque nada do que ele disser depois merece confiança
//! ([03, §8](../../../docs/03-protocolo.md)).
//!
//! Estes testes são um par hostil de verdade: não erro de transporte, mas alguém de propósito.
//! Quem recebe roda com privilégio e os bytes vêm da rede, então cada um deles tem de falhar.
//!
//! As recusas de política — cota, permissão, disco, caminho — estão em `recusa.rs`.

// Um teste de integração é um crate próprio, então a liberação que `#[cfg(test)]` concede à
// biblioteca não chega até aqui. Em teste, `unwrap` e `panic` com mensagem são o diagnóstico.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

mod comum;

use comum::{atravessar_com, conteudo_variado, escrever, ler_arvore, tem_montagem_parcial, temp};
use ir_files::error::FileError;
use ir_files::{Abertura, Cota, Reacao, Recepcao};
use ir_proto::message::{BulkMessage, CancelReason, ManifestItem, TransferId};

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
async fn um_bloco_maior_que_o_tamanho_declarado_derruba_o_enlace() {
    // O ataque mais direto contra a cota: anunciar pouco e mandar muito. Sem esta verificação, a
    // cota aprovada no manifesto não protegeria nada.
    let temp = temp("recusa-bloco-grande");
    let origem_dir = temp.sub("origem");
    let alvo = origem_dir.join("pequeno.bin");
    escrever(&alvo, &conteudo_variado(100)).await;
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
                data.extend_from_slice(&[0u8; 5000]);
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
    assert!(erro.derruba_o_enlace(), "{erro}");
    assert!(!tem_montagem_parcial(&recebidos));
    assert!(ler_arvore(&recebidos).is_empty());
}

#[tokio::test]
async fn um_deslocamento_fora_de_ordem_derruba_o_enlace() {
    // Sobre TCP a ordem é garantida, então deslocamento fora de lugar não é a rede reordenando.
    // Escrever em posição arbitrária deixaria um buraco no arquivo, e o resumo acusaria depois sem
    // dizer o porquê.
    let temp = temp("recusa-offset");
    let origem_dir = temp.sub("origem");
    let alvo = origem_dir.join("arquivo.bin");
    escrever(
        &alvo,
        &conteudo_variado(ir_proto::limits::MAX_FILE_BLOCK * 2),
    )
    .await;
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
                data,
            } => Some(BulkMessage::FileBlock {
                id,
                item,
                offset: offset + 7,
                data,
            }),
            outra => Some(outra),
        },
    )
    .await;

    assert!(fim.falha().derruba_o_enlace());
    assert!(!tem_montagem_parcial(&recebidos));
}

#[tokio::test]
async fn um_bloco_sem_arquivo_aberto_derruba_o_enlace() {
    let temp = temp("recusa-sem-inicio");
    let origem_dir = temp.sub("origem");
    let alvo = origem_dir.join("arquivo.bin");
    escrever(&alvo, &conteudo_variado(500)).await;
    let recebidos = temp.sub("recebidos");

    // O `FileStart` desaparece no caminho; o bloco chega sozinho.
    let fim = atravessar_com(&[alvo], &recebidos, Cota::default(), |_, mensagem| {
        if matches!(mensagem, BulkMessage::FileStart { .. }) {
            None
        } else {
            Some(mensagem)
        }
    })
    .await;

    assert!(fim.falha().derruba_o_enlace());
}

#[tokio::test]
async fn um_arquivo_que_termina_antes_do_tamanho_declarado_derruba_o_enlace() {
    // Um bloco some e o `FileEnd` chega de todo jeito. Sem esta verificação, um arquivo truncado
    // seria publicado como se estivesse inteiro — e o resumo até acusaria, mas pelo motivo errado.
    let temp = temp("recusa-truncado");
    let origem_dir = temp.sub("origem");
    let alvo = origem_dir.join("arquivo.bin");
    escrever(
        &alvo,
        &conteudo_variado(ir_proto::limits::MAX_FILE_BLOCK * 3),
    )
    .await;
    let recebidos = temp.sub("recebidos");

    let mut vistos = 0;
    let fim = atravessar_com(&[alvo], &recebidos, Cota::default(), |_, mensagem| {
        if matches!(mensagem, BulkMessage::FileBlock { .. }) {
            vistos += 1;
            if vistos == 2 {
                return None; // o segundo bloco se perde
            }
        }
        Some(mensagem)
    })
    .await;

    assert!(fim.falha().derruba_o_enlace());
    assert!(ler_arvore(&recebidos).is_empty());
}

#[tokio::test]
async fn um_item_fora_do_manifesto_derruba_o_enlace() {
    let temp = temp("recusa-item-inexistente");
    let origem_dir = temp.sub("origem");
    let alvo = origem_dir.join("arquivo.bin");
    escrever(&alvo, &conteudo_variado(50)).await;
    let recebidos = temp.sub("recebidos");

    let fim = atravessar_com(
        &[alvo],
        &recebidos,
        Cota::default(),
        |_, mensagem| match mensagem {
            BulkMessage::FileStart { id, .. } => Some(BulkMessage::FileStart { id, item: 9999 }),
            outra => Some(outra),
        },
    )
    .await;

    assert!(fim.falha().derruba_o_enlace());
}

#[tokio::test]
async fn abrir_um_diretorio_como_arquivo_derruba_o_enlace() {
    let temp = temp("recusa-dir-como-arquivo");
    let recebidos = temp.sub("recebidos");
    let itens = vec![item("pasta", 0, true), item("pasta/x.txt", 3, false)];

    let Abertura::Aceita { mut recepcao, .. } =
        abrir(&recebidos, itens, 3, Cota::default()).await.unwrap()
    else {
        panic!("o manifesto era válido");
    };
    let erro = recepcao
        .aplicar(BulkMessage::FileStart {
            id: TransferId(1),
            item: 0,
        })
        .await
        .unwrap_err();
    assert!(erro.derruba_o_enlace(), "{erro}");
}

#[tokio::test]
async fn mensagem_de_outra_transferencia_e_ignorada_e_nao_derruba_nada() {
    // Pode ser sobra de uma cópia que o usuário já substituiu, e o par tinha o direito de ter
    // mandado antes de saber. Derrubar o enlace por isso transformaria copiar duas vezes rápido
    // numa queda de conexão.
    let temp = temp("recusa-outra-transferencia");
    let recebidos = temp.sub("recebidos");
    let itens = vec![item("x.txt", 3, false)];

    let Abertura::Aceita { mut recepcao, .. } =
        abrir(&recebidos, itens, 3, Cota::default()).await.unwrap()
    else {
        panic!("o manifesto era válido");
    };
    let reacao = recepcao
        .aplicar(BulkMessage::FileStart {
            id: TransferId(77),
            item: 0,
        })
        .await
        .unwrap();
    assert!(matches!(reacao, Reacao::Nada), "{reacao:?}");
    assert_eq!(recepcao.escritos(), 0);
}

#[tokio::test]
async fn o_par_cancelando_nao_publica_nada() {
    let temp = temp("recusa-cancelado");
    let recebidos = temp.sub("recebidos");
    let itens = vec![item("x.txt", 3, false)];

    let raiz_da_montagem = {
        let Abertura::Aceita { mut recepcao, .. } =
            abrir(&recebidos, itens, 3, Cota::default()).await.unwrap()
        else {
            panic!("o manifesto era válido");
        };
        let reacao = recepcao
            .aplicar(BulkMessage::Cancel {
                id: TransferId(1),
                reason: CancelReason::UserRequested,
            })
            .await
            .unwrap();
        assert!(
            matches!(reacao, Reacao::Cancelada(CancelReason::UserRequested)),
            "{reacao:?}"
        );
        recebidos.join(".parcial-1")
    };
    assert!(
        !raiz_da_montagem.exists(),
        "a montagem tinha de ter ido embora com o `Drop`"
    );
    assert!(ler_arvore(&recebidos).is_empty());
}

#[tokio::test]
async fn publicar_com_item_por_conferir_e_recusado() {
    // A última barreira antes de o usuário ver o resultado: uma árvore incompleta não é a cópia
    // que ele pediu, e entregá-la seria pior que dizer que falhou.
    let temp = temp("recusa-incompleto");
    let recebidos = temp.sub("recebidos");
    let itens = vec![item("a.txt", 1, false), item("b.txt", 1, false)];

    let Abertura::Aceita { recepcao, .. } =
        abrir(&recebidos, itens, 2, Cota::default()).await.unwrap()
    else {
        panic!("o manifesto era válido");
    };
    let erro = recepcao.concluir().await.unwrap_err();
    assert!(erro.derruba_o_enlace(), "{erro}");
    assert!(ler_arvore(&recebidos).is_empty());
}
