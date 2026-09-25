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

/// O começo do nome de toda montagem dentro da pasta de recebidos.
///
/// Começa com ponto para o gerenciador de arquivos não a mostrar, e diz que está parcial.
const PREFIXO_DA_MONTAGEM: &str = ".parcial-";

/// Se uma entrada da pasta de recebidos, pelo nome, é uma montagem em curso — e não uma entrega.
///
/// A faxina precisa saber: tratar a montagem como entrega fazia "Limpar agora" apagar a cópia que
/// ainda estava chegando, e somava ao espaço ocupado o que nem tinha chegado inteiro.
#[must_use]
pub fn e_montagem(nome: &str) -> bool {
    nome.starts_with(PREFIXO_DA_MONTAGEM)
}

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
        let raiz = recebidos.join(format!("{PREFIXO_DA_MONTAGEM}{}", id.0));
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
            // `push` de um componente com raiz troca o caminho inteiro; só um nome comum entra.
            let mut componentes = Path::new(parte).components();
            match (componentes.next(), componentes.next()) {
                (Some(std::path::Component::Normal(_)), None) => destino.push(parte),
                _ => return Err(FileError::Violacao("caminho relativo inseguro")),
            }
        }
        if !destino.starts_with(&self.raiz) {
            return Err(FileError::Violacao("caminho relativo inseguro"));
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
        trocar(&self.raiz, &alvo).await?;
        // Só depois de o `rename` ter dado certo. Se ele falhar, o `Drop` ainda tem de limpar.
        self.publicado = true;
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
        trocar(&origem, &alvo).await?;
        // `publicado` fica falso de propósito: o que saiu foi o conteúdo, e a casca da montagem
        // ainda tem de ser recolhida.
        Ok(alvo)
    }
}

/// Põe `origem` no lugar de `alvo`: afasta a entrega anterior de mesmo nome, renomeia, e só então
/// apaga a anterior.
///
/// # Errors
///
/// [`FileError::Io`] se o `rename` falhar — e aí a anterior, afastada, fica no lugar dela.
async fn trocar(origem: &Path, alvo: &Path) -> Result<()> {
    let anterior = afastar_anterior(alvo).await?;
    tokio::fs::rename(origem, alvo)
        .await
        .map_err(|erro| FileError::io(alvo, erro))?;
    apagar(anterior).await;
    Ok(())
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

/// Apaga o que foi afastado. Falhar aqui só deixa lixo, e o serviço tem quem o recolha depois
/// (`ir_transferencia::faxina`).
async fn apagar(caminho: Option<PathBuf>) {
    if let Some(caminho) = caminho {
        let _ = remover(&caminho).await;
    }
}

/// Apaga um caminho da pasta de recebidos, seja arquivo ou árvore inteira.
///
/// Público porque a faxina apaga as mesmas coisas que a publicação afasta: uma regra só para
/// "apagar uma entrega".
///
/// # Errors
///
/// O erro do sistema ao apagar.
pub async fn remover(caminho: &Path) -> std::io::Result<()> {
    if tokio::fs::symlink_metadata(caminho)
        .await
        .is_ok_and(|dados| dados.is_dir())
    {
        tokio::fs::remove_dir_all(caminho).await
    } else {
        tokio::fs::remove_file(caminho).await
    }
}

/// Apaga as montagens que um serviço morto no meio de uma cópia deixou na pasta de recebidos, e
/// diz quantas eram.
///
/// Só na subida, quando nenhuma cópia pode estar em curso: durante a sessão a montagem é de quem
/// está recebendo, e a faxina não a toca ([`e_montagem`]). Sem isto, a de um processo morto só
/// saía se viesse outra cópia de mesmo identificador — ou seja, nunca.
pub async fn recolher_orfas(recebidos: &Path) -> usize {
    let Ok(mut leitura) = tokio::fs::read_dir(recebidos).await else {
        return 0;
    };
    let mut recolhidas = 0;
    while let Ok(Some(item)) = leitura.next_entry().await {
        let orfa = item.file_name().to_str().is_some_and(e_montagem);
        if orfa && remover(&item.path()).await.is_ok() {
            recolhidas += 1;
        }
    }
    recolhidas
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
mod testes;
