use super::*;
use crate::teste::pasta_temporaria;

async fn escrever(caminho: &Path, conteudo: &[u8]) {
    if let Some(pai) = caminho.parent() {
        tokio::fs::create_dir_all(pai).await.unwrap();
    }
    tokio::fs::write(caminho, conteudo).await.unwrap();
}

fn caminhos(plano: &Plano) -> Vec<String> {
    let mut todos: Vec<String> = plano.itens.iter().map(|i| i.path.clone()).collect();
    todos.sort();
    todos
}

#[tokio::test]
async fn um_arquivo_solto_vira_um_item() {
    let temp = pasta_temporaria("manifesto-um");
    let alvo = temp.caminho().join("nota.txt");
    escrever(&alvo, b"doze bytes..").await;

    let plano = montar(TransferId(1), &[alvo], Leitor::Proprio)
        .await
        .unwrap();
    assert_eq!(caminhos(&plano), vec!["nota.txt"]);
    assert_eq!(plano.total, 12);
    assert_eq!(plano.nome, "nota.txt");
}

#[tokio::test]
async fn uma_arvore_vira_pastas_e_arquivos_com_caminho_relativo() {
    let temp = pasta_temporaria("manifesto-arvore");
    let raiz = temp.caminho().join("relatorio");
    escrever(&raiz.join("a.pdf"), b"12345").await;
    escrever(&raiz.join("anexos").join("b.bin"), b"123").await;

    let plano = montar(TransferId(2), &[raiz], Leitor::Proprio)
        .await
        .unwrap();
    assert_eq!(
        caminhos(&plano),
        vec![
            "relatorio",
            "relatorio/a.pdf",
            "relatorio/anexos",
            "relatorio/anexos/b.bin",
        ]
    );
    assert_eq!(plano.total, 8, "só arquivo conta para o total");
    assert_eq!(plano.nome, "relatorio");
}

#[tokio::test]
async fn o_total_do_plano_e_aceito_pela_cota_sem_ajuste() {
    // As duas pontas do mesmo número: quem monta e quem confere. Se `montar` somasse
    // diretório, ou se `avaliar` não os descontasse, este teste falharia — e o sintoma real
    // seria uma transferência recusada por total que não bate.
    let temp = pasta_temporaria("manifesto-cota");
    let raiz = temp.caminho().join("pasta");
    escrever(&raiz.join("a").join("x.bin"), b"abcdefghij").await;

    let plano = montar(TransferId(3), &[raiz], Leitor::Proprio)
        .await
        .unwrap();
    assert_eq!(
        crate::cota::avaliar(
            &plano.itens,
            plano.total,
            crate::cota::Cota::default(),
            None
        )
        .unwrap(),
        None
    );
}

#[tokio::test]
async fn varias_raizes_entram_lado_a_lado() {
    let temp = pasta_temporaria("manifesto-varias");
    let a = temp.caminho().join("a.txt");
    let b = temp.caminho().join("b.txt");
    escrever(&a, b"a").await;
    escrever(&b, b"bb").await;

    let plano = montar(TransferId(4), &[a, b], Leitor::Proprio)
        .await
        .unwrap();
    assert_eq!(caminhos(&plano), vec!["a.txt", "b.txt"]);
    assert_eq!(plano.total, 3);
    assert_eq!(plano.nome, "a.txt e outros");
}

#[tokio::test]
async fn uma_pasta_vazia_ainda_e_um_item() {
    // Copiar uma pasta vazia e receber nada seria perda silenciosa.
    let temp = pasta_temporaria("manifesto-vazia");
    let raiz = temp.caminho().join("vazia");
    tokio::fs::create_dir_all(&raiz).await.unwrap();

    let plano = montar(TransferId(5), &[raiz], Leitor::Proprio)
        .await
        .unwrap();
    assert_eq!(caminhos(&plano), vec!["vazia"]);
    assert_eq!(plano.total, 0);
}

#[tokio::test]
async fn o_que_nao_existe_e_erro_e_nao_um_plano_vazio() {
    let temp = pasta_temporaria("manifesto-ausente");
    let erro = montar(
        TransferId(6),
        &[temp.caminho().join("nao-existe")],
        Leitor::Proprio,
    )
    .await
    .unwrap_err();
    assert!(matches!(erro, FileError::NaoEnviavel(_)), "{erro}");
}

#[tokio::test]
async fn todo_caminho_do_manifesto_e_seguro_para_o_destino() {
    // A propriedade que fecha o ciclo: o que este módulo produz é exatamente o que o
    // `is_safe_path` do destino aceita. Se as duas regras divergirem, a transferência é
    // recusada por caminho inseguro que nós mesmos montamos.
    let temp = pasta_temporaria("manifesto-seguro");
    let raiz = temp.caminho().join("com espaço e acentuação");
    escrever(&raiz.join("sub pasta").join("arquivo (1).txt"), b"x").await;

    let plano = montar(TransferId(7), &[raiz], Leitor::Proprio)
        .await
        .unwrap();
    assert!(!plano.vazio());
    for item in &plano.itens {
        assert!(item.is_safe_path(), "{}", item.path);
        assert!(!item.path.contains('\\'), "{}", item.path);
    }
}

#[tokio::test]
async fn os_dois_vetores_andam_juntos() {
    let temp = pasta_temporaria("manifesto-paralelo");
    let raiz = temp.caminho().join("p");
    escrever(&raiz.join("a.txt"), b"1").await;
    escrever(&raiz.join("b.txt"), b"22").await;

    let plano = montar(TransferId(8), &[raiz], Leitor::Proprio)
        .await
        .unwrap();
    assert_eq!(
        plano.itens.len(),
        plano.locais.len(),
        "um item sem caminho local é um bloco que não sabe o que ler"
    );
    for (item, local) in plano.itens.iter().zip(&plano.locais) {
        assert!(
            local.ends_with(item.path.rsplit('/').next().unwrap_or_default()),
            "{} não corresponde a {local:?}",
            item.path
        );
    }
}

#[cfg(unix)]
#[tokio::test]
async fn vinculo_simbolico_e_ignorado_e_contado() {
    // Um laço faria a varredura não terminar; um vínculo para fora copiaria o que o usuário
    // não selecionou. Ignorar é a resposta, e dizer quantos foi ignorado é o mínimo.
    let temp = pasta_temporaria("manifesto-vinculo");
    let raiz = temp.caminho().join("p");
    escrever(&raiz.join("real.txt"), b"x").await;
    std::os::unix::fs::symlink(&raiz, raiz.join("laco")).unwrap();

    let plano = montar(TransferId(9), &[raiz], Leitor::Proprio)
        .await
        .unwrap();
    assert_eq!(caminhos(&plano), vec!["p", "p/real.txt"]);
    assert_eq!(plano.ignorados, 1);
}

#[test]
fn so_caminhos_absolutos_desta_maquina_entram() {
    assert!(!caminho_local(Path::new("relativo/a.txt")));
    #[cfg(windows)]
    {
        assert!(caminho_local(Path::new(r"C:\Users\a\b.txt")));
        assert!(caminho_local(Path::new(r"\\?\C:\Users\a\b.txt")));
        // Perguntar por qualquer um destes faria o SYSTEM se autenticar no servidor dado.
        for rede in [
            r"\\servidor\pasta\a.txt",
            r"\\?\UNC\servidor\pasta\a.txt",
            r"\\.\pipe\algo",
        ] {
            assert!(!caminho_local(Path::new(rede)), "{rede}");
        }
    }
    #[cfg(not(windows))]
    assert!(caminho_local(Path::new("/home/a/b.txt")));
}

#[tokio::test]
async fn um_caminho_de_rede_e_recusado_antes_de_tocar_o_disco() {
    #[cfg(windows)]
    let rede = PathBuf::from(r"\\192.0.2.1\c$\a.txt");
    #[cfg(not(windows))]
    let rede = PathBuf::from("relativo.txt");
    let erro = montar(TransferId(8), &[rede], Leitor::Proprio)
        .await
        .unwrap_err();
    #[cfg(windows)]
    assert!(matches!(erro, FileError::PastaDeRede(_)), "{erro}");
    #[cfg(not(windows))]
    assert!(matches!(erro, FileError::NaoEnviavel(_)), "{erro}");
}

#[test]
fn so_arquivo_comum_abre_para_enviar() {
    let temp = pasta_temporaria("abrir-para-enviar");
    let arquivo = temp.caminho().join("a.txt");
    std::fs::write(&arquivo, b"x").unwrap();
    assert!(crate::envio::abrir_para_enviar(&arquivo).is_ok());
    assert!(matches!(
        crate::envio::abrir_para_enviar(temp.caminho()),
        Err(FileError::NaoEnviavel(_) | FileError::Io { .. })
    ));
}

#[cfg(target_os = "linux")]
#[test]
fn um_fifo_trocado_no_lugar_do_arquivo_nao_prende_o_servico() {
    let temp = pasta_temporaria("abrir-fifo");
    let fifo = temp.caminho().join("f");
    let feito = std::process::Command::new("mkfifo").arg(&fifo).status();
    if !feito.is_ok_and(|s| s.success()) {
        return; // sem `mkfifo` nesta máquina
    }
    // Com o `open` comum isto não voltava nunca.
    let erro = crate::envio::abrir_para_enviar(&fifo).unwrap_err();
    assert!(matches!(erro, FileError::NaoEnviavel(_)), "{erro}");
}
