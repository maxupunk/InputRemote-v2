//! A área de montagem, e a promessa de que ela não sobrevive a um fracasso.
//!
//! O critério de saída da Etapa 8 é explícito: *"cancelamento no meio não deixa arquivo temporário
//! nem árvore parcial"*. Isso não é uma sequência de limpeza a ser chamada nos caminhos de erro —
//! é um invariante, e caminho de erro é exatamente onde a chamada de limpeza é esquecida.
//!
//! Por isso a limpeza está no `Drop`. Uma transferência que termina bem chama [`Staging::publicar`]
//! e a árvore muda de nome; **qualquer** outro fim — erro de rede, resumo divergente, cancelamento
//! do usuário, o processo saindo de escopo por um `?` num ramo que ninguém previu — apaga tudo,
//! porque apagar é o que acontece quando não se faz nada.
//!
//! # Montar ao lado do destino, e não em `/tmp`
//!
//! A publicação é um `rename`, e `rename` só é atômico e barato dentro do mesmo sistema de
//! arquivos. Montar em `/tmp` e depois copiar para o destino significaria escrever 5 GB duas
//! vezes, e a segunda escrita não teria a atomicidade que é o ponto todo.
//!
//! Então a montagem acontece **dentro** da pasta de recebidos, num diretório cujo nome começa com
//! ponto e diz que está parcial.

use std::path::{Path, PathBuf};

use ir_proto::message::{ManifestItem, TransferId};
use tracing::debug;

use crate::error::{FileError, Result};

/// O sufixo da entrega anterior enquanto ela é trocada pela nova.
///
/// Nome improvável de propósito: ele existe por instantes, entre o `rename` que tira a anterior do
/// caminho e o que põe a nova no lugar.
const ANTERIOR: &str = "anterior-do-inputremote";

/// Uma árvore a meio caminho, que se apaga sozinha se não for publicada.
#[derive(Debug)]
pub struct Staging {
    raiz: PathBuf,
    publicado: bool,
}

impl Staging {
    /// Cria a área de montagem dentro de `recebidos`.
    ///
    /// # Errors
    ///
    /// [`FileError::Io`] se a pasta não puder ser criada.
    pub async fn criar(recebidos: &Path, id: TransferId) -> Result<Self> {
        tokio::fs::create_dir_all(recebidos)
            .await
            .map_err(|erro| FileError::io(recebidos, erro))?;
        let raiz = recebidos.join(format!(".parcial-{}", id.0));
        // Uma montagem anterior pode ter ficado para trás se o processo foi morto sem `Drop` —
        // um `kill -9`, uma queda de energia. Recomeçar do zero é o único estado conhecido.
        if tokio::fs::metadata(&raiz).await.is_ok() {
            debug!(?raiz, "havia uma montagem anterior; recomeçando do zero");
            tokio::fs::remove_dir_all(&raiz)
                .await
                .map_err(|erro| FileError::io(&raiz, erro))?;
        }
        tokio::fs::create_dir(&raiz)
            .await
            .map_err(|erro| FileError::io(&raiz, erro))?;
        Ok(Self {
            raiz,
            publicado: false,
        })
    }

    /// A raiz da montagem.
    #[must_use]
    pub fn raiz(&self) -> &Path {
        &self.raiz
    }

    /// O caminho local de um item do manifesto, conferindo a segurança **de novo**.
    ///
    /// A cota já recusou manifesto com caminho de fuga. Este segundo exame não é desconfiança da
    /// cota: é que este é o ponto onde um caminho vira uma escrita em disco feita por um processo
    /// privilegiado, e a verificação pertence a onde a consequência está. Se algum dia alguém
    /// montar uma recepção sem passar pela cota, ela não escreve fora daqui.
    ///
    /// # Errors
    ///
    /// [`FileError::Violacao`] se o caminho relativo não for seguro.
    pub fn caminho_de(&self, item: &ManifestItem) -> Result<PathBuf> {
        self.caminho_seguro(&item.path)
    }

    /// O mesmo, a partir de um caminho relativo solto.
    ///
    /// A regra de segurança vive no `ir-proto`, com o tipo do fio, e não é reescrita aqui: duas
    /// cópias da mesma regra divergem, e esta é a regra que impede escrita fora do destino.
    fn caminho_seguro(&self, relativo: &str) -> Result<PathBuf> {
        let como_item = ManifestItem {
            path: relativo.to_owned(),
            size: 0,
            is_dir: false,
        };
        if !como_item.is_safe_path() {
            return Err(FileError::Violacao("caminho relativo inseguro"));
        }
        let mut destino = self.raiz.clone();
        for parte in relativo.split('/') {
            destino.push(parte);
        }
        Ok(destino)
    }

    /// Cria o diretório de um item, e os pais que faltarem.
    ///
    /// # Errors
    ///
    /// [`FileError::Violacao`] para caminho inseguro; [`FileError::Io`] em falha de escrita.
    pub async fn criar_pasta(&self, item: &ManifestItem) -> Result<()> {
        let destino = self.caminho_de(item)?;
        tokio::fs::create_dir_all(&destino)
            .await
            .map_err(|erro| FileError::io(&destino, erro))
    }

    /// Garante que a pasta que conterá um arquivo existe.
    ///
    /// Existe porque o manifesto **não garante ordem**: um `FileStart` de `a/b/c.txt` pode chegar
    /// antes do item de diretório `a/b`, ou o diretório pode nem estar no manifesto. Depender da
    /// ordem seria depender de um detalhe do emissor.
    ///
    /// # Errors
    ///
    /// [`FileError::Violacao`] para caminho inseguro; [`FileError::Io`] em falha de escrita.
    pub async fn preparar_pai(&self, item: &ManifestItem) -> Result<PathBuf> {
        let destino = self.caminho_de(item)?;
        if let Some(pai) = destino.parent() {
            tokio::fs::create_dir_all(pai)
                .await
                .map_err(|erro| FileError::io(pai, erro))?;
        }
        Ok(destino)
    }

    /// Publica a montagem inteira com o nome pedido — **o nome pedido**, sem sufixo.
    ///
    /// Um `rename` só: até a última linha não havia nada visível no destino, e depois dela está
    /// tudo. Não existe instante em que o usuário veja meia árvore.
    ///
    /// # Errors
    ///
    /// [`FileError::Io`] se o `rename` falhar.
    pub async fn publicar(mut self, recebidos: &Path, nome: &str) -> Result<PathBuf> {
        let alvo = recebidos.join(nome);
        let anterior = afastar_anterior(&alvo).await?;
        tokio::fs::rename(&self.raiz, &alvo)
            .await
            .map_err(|erro| FileError::io(&alvo, erro))?;
        // Só depois de o `rename` ter dado certo. Se ele falhar, o `Drop` ainda tem de limpar.
        self.publicado = true;
        apagar(anterior).await;
        Ok(alvo)
    }

    /// Publica **uma entrada de dentro** da montagem, e não a montagem.
    ///
    /// # Por que isto existe
    ///
    /// Quando o usuário copia a pasta `relatório`, o manifesto descreve `relatório`,
    /// `relatório/a.pdf`, e a montagem fica com essa árvore dentro dela. Renomear a montagem para
    /// `recebidos/relatório` produziria `recebidos/relatório/relatório/a.pdf` — um nível a mais,
    /// que o usuário vê e não entende.
    ///
    /// Publicando a entrada de dentro, o destino espelha a origem exatamente. A montagem sobra
    /// vazia e o `Drop` a recolhe, como recolheria qualquer outra sobra.
    ///
    /// # Errors
    ///
    /// [`FileError::Violacao`] se o caminho relativo não for seguro; [`FileError::Io`] se nenhum
    /// nome livre for encontrado ou o `rename` falhar.
    pub async fn publicar_dentro(self, recebidos: &Path, relativo: &str) -> Result<PathBuf> {
        let origem = self.caminho_seguro(relativo)?;
        let nome = relativo
            .rsplit('/')
            .next()
            .filter(|nome| !nome.is_empty())
            .ok_or(FileError::Violacao("entrada sem nome para publicar"))?;
        let alvo = recebidos.join(nome);
        let anterior = afastar_anterior(&alvo).await?;
        tokio::fs::rename(&origem, &alvo)
            .await
            .map_err(|erro| FileError::io(&alvo, erro))?;
        // `publicado` fica falso de propósito: o que saiu foi o conteúdo, e a casca da montagem
        // ainda tem de ser recolhida.
        apagar(anterior).await;
        Ok(alvo)
    }
}

/// Tira do caminho a entrega anterior de mesmo nome, e diz para onde ela foi.
///
/// O que chega é **a versão nova daquilo**, e é o nome dela que o usuário vai colar. Acrescentar
/// "(2)" fazia o arquivo chegar do outro lado com outro nome — a queixa do usuário —, e ainda
/// deixava as duas cópias ocupando disco.
///
/// A anterior é afastada, e não apagada de uma vez: se o `rename` da nova falhar, o usuário fica
/// com a antiga, que é melhor que ficar sem nenhuma. Quem apaga é [`apagar`], depois do sucesso.
async fn afastar_anterior(alvo: &Path) -> Result<Option<PathBuf>> {
    if tokio::fs::metadata(alvo).await.is_err() {
        return Ok(None);
    }
    let nome = alvo.file_name().map_or_else(
        || ANTERIOR.to_owned(),
        |nome| nome.to_string_lossy().into_owned(),
    );
    let afastado = alvo.with_file_name(format!("{nome}.{ANTERIOR}"));
    // Uma sobra de uma tentativa anterior não pode impedir esta entrega.
    if tokio::fs::metadata(&afastado).await.is_ok() {
        apagar(Some(afastado.clone())).await;
    }
    if tokio::fs::rename(alvo, &afastado).await.is_err() {
        // Não deu para afastar (outro processo com o arquivo aberto, no Windows): apagar direto é
        // a única saída, e é o que o usuário espera de "a versão nova daquilo".
        apagar(Some(alvo.to_path_buf())).await;
        return Ok(None);
    }
    Ok(Some(afastado))
}

/// Apaga o que foi afastado, seja arquivo ou árvore. Falhar aqui só deixa lixo, e o serviço tem
/// quem o recolha depois (`ir_transferencia::faxina`).
async fn apagar(caminho: Option<PathBuf>) {
    let Some(caminho) = caminho else { return };
    if tokio::fs::metadata(&caminho)
        .await
        .is_ok_and(|dados| dados.is_dir())
    {
        let _ = tokio::fs::remove_dir_all(&caminho).await;
        return;
    }
    let _ = tokio::fs::remove_file(&caminho).await;
}

impl Drop for Staging {
    fn drop(&mut self) {
        if self.publicado {
            return;
        }
        // Síncrono de propósito: `Drop` não pode esperar, e a garantia de não deixar árvore
        // parcial vale mais que não bloquear por alguns milissegundos num caminho de falha.
        match std::fs::remove_dir_all(&self.raiz) {
            Ok(()) => debug!(raiz = ?self.raiz, "montagem incompleta apagada"),
            Err(erro) if erro.kind() == std::io::ErrorKind::NotFound => {}
            Err(erro) => debug!(%erro, raiz = ?self.raiz, "não consegui apagar a montagem"),
        }
    }
}

#[cfg(test)]
mod tests {
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
}
