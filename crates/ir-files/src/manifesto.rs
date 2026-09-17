//! Montar o manifesto: olhar o que o usuário copiou e descrever a árvore **antes** de mandar um
//! byte.
//!
//! O manifesto primeiro é o que permite ao destino conferir cota, permissão e espaço e recusar de
//! imediato, em vez de descobrir no meio e deixar uma árvore parcial
//! ([01, §3.3](../../../docs/01-visao-e-escopo.md)).
//!
//! # Três decisões sobre o que **não** entra
//!
//! **Vínculo simbólico não é seguido nem incluído.** Seguir abriria dois problemas de uma vez: um
//! laço (`a` aponta para `.`) faz a varredura não terminar, e um vínculo para fora da árvore copia
//! o que o usuário não selecionou — `~/.ssh`, por exemplo, se alguém puser um atalho numa pasta
//! copiada. Incluir sem seguir exigiria representar o alvo no protocolo, e o canal 5 não tem essa
//! mensagem. Então são ignorados, e a contagem de ignorados é devolvida para que a interface possa
//! dizer.
//!
//! **Nome que não é UTF-8 não entra.** O campo do fio é `String`. No Linux um nome de arquivo é uma
//! sequência de bytes que pode não ser texto válido; mandar uma substituição silenciosa criaria
//! arquivo com nome diferente do original no destino, e é pior que dizer que não deu.
//!
//! **Profundidade tem teto.** Não por medo de recursão — a varredura é iterativa —, mas porque uma
//! árvore de dez mil níveis é sintoma, não dado.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};

use ir_proto::limits;
use ir_proto::message::{ManifestItem, TransferId};

use crate::error::{FileError, Result};

/// Profundidade máxima de diretórios dentro de uma raiz copiada.
const PROFUNDIDADE_MAXIMA: usize = 64;

/// O que enviar, já conferido contra os limites.
#[derive(Debug, Clone)]
pub struct Plano {
    /// Identificador desta transferência.
    pub id: TransferId,
    /// Os itens, como vão no manifesto.
    pub itens: Vec<ManifestItem>,
    /// O caminho local de cada item, na mesma ordem de [`Self::itens`].
    ///
    /// Dois vetores paralelos em vez de um campo dentro do item porque `ManifestItem` é tipo de
    /// fio: o caminho local **não deve** ser representável nele. O par não tem nada que saber onde
    /// os arquivos moram nesta máquina.
    pub locais: Vec<PathBuf>,
    /// Soma dos tamanhos dos arquivos.
    pub total: u64,
    /// Nome sugerido para a pasta de destino.
    pub nome: String,
    /// Quantas entradas foram ignoradas, e por quê contadas juntas.
    pub ignorados: usize,
}

impl Plano {
    /// Se não há nada a enviar.
    #[must_use]
    pub fn vazio(&self) -> bool {
        self.itens.is_empty()
    }
}

/// Monta o plano de envio a partir do que o usuário copiou.
///
/// # Errors
///
/// [`FileError::NaoEnviavel`] se uma das raízes não existe ou não é arquivo nem diretório;
/// [`FileError::CaminhoImpossivel`] para nome que não é UTF-8; [`FileError::Io`] em falha de
/// leitura de diretório; [`FileError::Violacao`] se a árvore passa dos limites do protocolo — e
/// aqui a violação é nossa, não do par, o que é justamente por que ela é pega antes de enviar.
pub async fn montar(id: TransferId, raizes: &[PathBuf]) -> Result<Plano> {
    let mut plano = Plano {
        id,
        itens: Vec::new(),
        locais: Vec::new(),
        total: 0,
        nome: nome_do_destino(raizes),
        ignorados: 0,
    };
    for raiz in raizes {
        acrescentar_raiz(&mut plano, raiz).await?;
    }
    Ok(plano)
}

/// Acrescenta uma raiz — arquivo solto ou árvore inteira.
async fn acrescentar_raiz(plano: &mut Plano, raiz: &Path) -> Result<()> {
    // `symlink_metadata` e não `metadata`: aqui a pergunta é "o que é esta entrada", e não "o que
    // há no fim do vínculo".
    let dados = tokio::fs::symlink_metadata(raiz)
        .await
        .map_err(|_| FileError::NaoEnviavel(raiz.to_path_buf()))?;
    if dados.is_symlink() {
        plano.ignorados += 1;
        return Ok(());
    }
    let nome = nome_relativo(raiz)?;
    if dados.is_file() {
        return acrescentar(plano, raiz, nome, dados.len(), false);
    }
    if !dados.is_dir() {
        return Err(FileError::NaoEnviavel(raiz.to_path_buf()));
    }
    acrescentar(plano, raiz, nome.clone(), 0, true)?;
    percorrer(plano, raiz, &nome).await
}

/// Percorre uma árvore, de forma iterativa e com teto de profundidade.
async fn percorrer(plano: &mut Plano, raiz: &Path, prefixo: &str) -> Result<()> {
    let mut fila: VecDeque<(PathBuf, String, usize)> =
        VecDeque::from([(raiz.to_path_buf(), prefixo.to_owned(), 0usize)]);

    while let Some((pasta, prefixo, nivel)) = fila.pop_front() {
        if nivel >= PROFUNDIDADE_MAXIMA {
            return Err(FileError::Violacao("árvore mais profunda que o teto"));
        }
        let mut entradas = tokio::fs::read_dir(&pasta)
            .await
            .map_err(|erro| FileError::io(&pasta, erro))?;
        while let Some(entrada) = entradas
            .next_entry()
            .await
            .map_err(|erro| FileError::io(&pasta, erro))?
        {
            let caminho = entrada.path();
            let dados = tokio::fs::symlink_metadata(&caminho)
                .await
                .map_err(|erro| FileError::io(&caminho, erro))?;
            if dados.is_symlink() {
                plano.ignorados += 1;
                continue;
            }
            let relativo = format!("{prefixo}/{}", nome_relativo(&caminho)?);
            if dados.is_dir() {
                acrescentar(plano, &caminho, relativo.clone(), 0, true)?;
                fila.push_back((caminho, relativo, nivel + 1));
            } else if dados.is_file() {
                acrescentar(plano, &caminho, relativo, dados.len(), false)?;
            } else {
                // Soquete, dispositivo, `fifo`. Não há o que copiar.
                plano.ignorados += 1;
            }
        }
    }
    Ok(())
}

/// Registra um item, conferindo os limites do protocolo antes.
fn acrescentar(
    plano: &mut Plano,
    local: &Path,
    relativo: String,
    tamanho: u64,
    pasta: bool,
) -> Result<()> {
    if plano.itens.len() >= limits::MAX_MANIFEST_ITEMS {
        return Err(FileError::Violacao("itens demais para um manifesto"));
    }
    let item = ManifestItem {
        path: relativo,
        size: if pasta { 0 } else { tamanho },
        is_dir: pasta,
    };
    if !item.is_safe_path() {
        return Err(FileError::CaminhoImpossivel(local.to_path_buf()));
    }
    if !pasta {
        plano.total = plano
            .total
            .checked_add(tamanho)
            .ok_or(FileError::Violacao("a soma dos tamanhos estourou"))?;
    }
    plano.itens.push(item);
    plano.locais.push(local.to_path_buf());
    Ok(())
}

/// O último componente de um caminho, como texto.
fn nome_relativo(caminho: &Path) -> Result<String> {
    caminho
        .file_name()
        .and_then(|nome| nome.to_str())
        .filter(|nome| !nome.is_empty() && *nome != "." && *nome != "..")
        .map(str::to_owned)
        .ok_or_else(|| FileError::CaminhoImpossivel(caminho.to_path_buf()))
}

/// Nome da pasta em que a entrega aparece no destino.
///
/// Uma raiz só empresta o próprio nome, que é o que o usuário reconhece. Várias raízes não têm um
/// nome natural, e inventar um do primeiro item seria enganoso quando há dez.
fn nome_do_destino(raizes: &[PathBuf]) -> String {
    match raizes {
        [uma] => nome_relativo(uma).unwrap_or_else(|_| "recebido".to_owned()),
        [primeiro, ..] => match nome_relativo(primeiro) {
            Ok(nome) => format!("{nome} e outros"),
            Err(_) => "recebidos".to_owned(),
        },
        [] => "recebido".to_owned(),
    }
}

#[cfg(test)]
mod tests {
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

        let plano = montar(TransferId(1), &[alvo]).await.unwrap();
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

        let plano = montar(TransferId(2), &[raiz]).await.unwrap();
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

        let plano = montar(TransferId(3), &[raiz]).await.unwrap();
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

        let plano = montar(TransferId(4), &[a, b]).await.unwrap();
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

        let plano = montar(TransferId(5), &[raiz]).await.unwrap();
        assert_eq!(caminhos(&plano), vec!["vazia"]);
        assert_eq!(plano.total, 0);
    }

    #[tokio::test]
    async fn o_que_nao_existe_e_erro_e_nao_um_plano_vazio() {
        let temp = pasta_temporaria("manifesto-ausente");
        let erro = montar(TransferId(6), &[temp.caminho().join("nao-existe")])
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

        let plano = montar(TransferId(7), &[raiz]).await.unwrap();
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

        let plano = montar(TransferId(8), &[raiz]).await.unwrap();
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

        let plano = montar(TransferId(9), &[raiz]).await.unwrap();
        assert_eq!(caminhos(&plano), vec!["p", "p/real.txt"]);
        assert_eq!(plano.ignorados, 1);
    }
}
