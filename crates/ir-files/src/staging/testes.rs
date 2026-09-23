use super::*;
use crate::teste::pasta_temporaria;

fn arquivo(caminho: &str) -> ManifestItem {
    ManifestItem {
        path: caminho.to_owned(),
        size: 0,
        is_dir: false,
    }
}

#[tokio::test]
async fn a_montagem_incompleta_desaparece_sozinha() {
    // O critério de saída da Etapa 8, como teste: nada de árvore parcial.
    let temp = pasta_temporaria("staging-descarta");
    let raiz = {
        let staging = Staging::criar(temp.caminho(), TransferId(7)).await.unwrap();
        let caminho = staging.preparar_pai(&arquivo("a/b/c.txt")).await.unwrap();
        tokio::fs::write(&caminho, b"parcial").await.unwrap();
        assert!(tokio::fs::metadata(&caminho).await.is_ok());
        staging.raiz().to_path_buf()
    };
    assert!(
        tokio::fs::metadata(&raiz).await.is_err(),
        "a montagem tinha de ter ido embora com o `Drop`"
    );
}

#[tokio::test]
async fn publicar_e_um_rename_e_o_drop_nao_apaga_o_publicado() {
    let temp = pasta_temporaria("staging-publica");
    let publicado = {
        let staging = Staging::criar(temp.caminho(), TransferId(1)).await.unwrap();
        let caminho = staging
            .preparar_pai(&arquivo("relatorio/a.txt"))
            .await
            .unwrap();
        tokio::fs::write(&caminho, b"conteudo").await.unwrap();
        staging.publicar(temp.caminho(), "entrega").await.unwrap()
    };
    let dentro = publicado.join("relatorio").join("a.txt");
    assert_eq!(tokio::fs::read(&dentro).await.unwrap(), b"conteudo");
}

/// A regra mudou em 2026-09-21, a pedido de quem usa: o recebido mantém **o nome**, e a
/// entrega nova toma o lugar da anterior de mesmo nome.
///
/// Antes cada entrega ganhava um sufixo — `entrega`, `entrega (2)`, `entrega (3)` — e o nome
/// alterado viajava para o outro lado na hora de colar: o usuário copiava `FIMI0022.LRV` e
/// colava `FIMI0022.LRV (2)`. E as versões velhas ficavam todas em disco.
#[tokio::test]
async fn a_entrega_nova_mantem_o_nome_e_toma_o_lugar_da_anterior() {
    let temp = pasta_temporaria("staging-mesmo-nome");
    let mut nomes = Vec::new();
    for n in 1..=3u32 {
        let staging = Staging::criar(temp.caminho(), TransferId(n)).await.unwrap();
        let caminho = staging.preparar_pai(&arquivo("x.txt")).await.unwrap();
        tokio::fs::write(&caminho, format!("versao {n}"))
            .await
            .unwrap();
        nomes.push(staging.publicar(temp.caminho(), "entrega").await.unwrap());
    }
    // O mesmo caminho nas três vezes: sem "(2)", sem "(3)".
    assert!(nomes.iter().all(|nome| *nome == nomes[0]), "{nomes:?}");
    assert_eq!(nomes[0].file_name().unwrap(), "entrega");
    // E o conteúdo é o da última.
    assert_eq!(
        tokio::fs::read_to_string(nomes[0].join("x.txt"))
            .await
            .unwrap(),
        "versao 3"
    );
    // Nada de sobra ao lado: nem "(2)", nem a pasta afastada.
    let mut restantes = Vec::new();
    let mut leitura = tokio::fs::read_dir(temp.caminho()).await.unwrap();
    while let Ok(Some(item)) = leitura.next_entry().await {
        restantes.push(item.file_name().to_string_lossy().into_owned());
    }
    assert_eq!(restantes, vec!["entrega".to_owned()], "{restantes:?}");
}

#[tokio::test]
async fn um_caminho_de_fuga_e_recusado_na_hora_de_escrever() {
    // A segunda linha de defesa: mesmo que a cota não tenha sido consultada, nada é escrito
    // fora da montagem.
    let temp = pasta_temporaria("staging-fuga");
    let staging = Staging::criar(temp.caminho(), TransferId(2)).await.unwrap();
    for fuga in ["../fora.txt", "/etc/passwd", "a/../../fora", "a\\b"] {
        let erro = staging.caminho_de(&arquivo(fuga)).unwrap_err();
        assert!(erro.derruba_o_enlace(), "{fuga}: {erro}");
    }
}

#[tokio::test]
async fn o_caminho_montado_fica_dentro_da_raiz() {
    let temp = pasta_temporaria("staging-dentro");
    let staging = Staging::criar(temp.caminho(), TransferId(3)).await.unwrap();
    let caminho = staging.caminho_de(&arquivo("a/b/c.txt")).unwrap();
    assert!(caminho.starts_with(staging.raiz()));
    assert!(caminho.ends_with("c.txt"));
}

#[tokio::test]
async fn uma_montagem_esquecida_por_um_kill_e_recomecada_do_zero() {
    // `Drop` não roda num `kill -9`. Ao subir de novo, a sobra não pode virar conteúdo da
    // transferência nova.
    let temp = pasta_temporaria("staging-sobra");
    let sobra = temp.caminho().join(".parcial-9");
    tokio::fs::create_dir_all(sobra.join("lixo")).await.unwrap();
    tokio::fs::write(sobra.join("lixo").join("velho.txt"), b"de antes")
        .await
        .unwrap();

    let staging = Staging::criar(temp.caminho(), TransferId(9)).await.unwrap();
    assert!(
        tokio::fs::metadata(staging.raiz().join("lixo"))
            .await
            .is_err(),
        "a sobra tinha de ter sido apagada"
    );
}

#[tokio::test]
async fn criar_pasta_faz_os_pais_que_faltam() {
    let temp = pasta_temporaria("staging-pais");
    let staging = Staging::criar(temp.caminho(), TransferId(4)).await.unwrap();
    let item = ManifestItem {
        path: "a/b/c".to_owned(),
        size: 0,
        is_dir: true,
    };
    staging.criar_pasta(&item).await.unwrap();
    assert!(
        tokio::fs::metadata(staging.raiz().join("a").join("b").join("c"))
            .await
            .is_ok()
    );
}

#[tokio::test]
async fn um_caminho_com_unidade_no_meio_nao_sai_da_montagem() {
    // O ataque que escapava: `x/C:payload.dll` virava `C:payload.dll`, relativo à pasta de
    // trabalho do serviço — `System32`, no Windows.
    let temp = pasta_temporaria("staging-unidade");
    let staging = Staging::criar(temp.caminho(), TransferId(9)).await.unwrap();
    for caminho in ["x/C:payload.dll", "a/b:fluxo", "a/NUL", "a/../../fora"] {
        let erro = staging.caminho_de(&arquivo(caminho)).unwrap_err();
        assert!(matches!(erro, FileError::Violacao(_)), "{caminho}: {erro}");
    }
    let bom = staging.caminho_de(&arquivo("a/b/c.txt")).unwrap();
    assert!(bom.starts_with(staging.raiz()));
}
