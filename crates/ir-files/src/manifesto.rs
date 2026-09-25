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
use crate::permissao::{self, Leitor};

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
    /// O nome da entrega, pela regra que o destino também usa
    /// ([`crate::publicacao::nome_da_entrega`]).
    pub nome: String,
    /// Quantas entradas foram ignoradas, e por quê contadas juntas.
    pub ignorados: usize,
    /// Com a autoridade de quem os arquivos serão lidos no envio.
    pub leitor: Leitor,
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
/// [`FileError::SemPermissao`] se alguma entrada não seria legível por `leitor`
/// ([`crate::permissao`]).
pub async fn montar(id: TransferId, raizes: &[PathBuf], leitor: Leitor) -> Result<Plano> {
    let mut plano = Plano {
        id,
        itens: Vec::new(),
        locais: Vec::new(),
        total: 0,
        nome: String::new(),
        ignorados: 0,
        leitor,
    };
    for raiz in raizes {
        acrescentar_raiz(&mut plano, raiz).await?;
    }
    // Do manifesto pronto, e não das raízes pedidas: é o que o destino vê, e só assim os dois lados
    // dão à mesma cópia o mesmo nome.
    plano.nome = crate::publicacao::nome_da_entrega(&plano.itens);
    Ok(plano)
}

/// Acrescenta uma raiz — arquivo solto ou árvore inteira.
async fn acrescentar_raiz(plano: &mut Plano, raiz: &Path) -> Result<()> {
    // Antes de qualquer acesso ao disco: perguntar por um caminho de rede já é o ataque.
    if !caminho_local(raiz) {
        return Err(if raiz.is_absolute() {
            FileError::PastaDeRede(raiz.to_path_buf())
        } else {
            FileError::NaoEnviavel(raiz.to_path_buf())
        });
    }
    // `symlink_metadata` e não `metadata`: aqui a pergunta é "o que é esta entrada", e não "o que
    // há no fim do vínculo".
    let dados = tokio::fs::symlink_metadata(raiz)
        .await
        .map_err(|_| FileError::NaoEnviavel(raiz.to_path_buf()))?;
    if dados.is_symlink() {
        plano.ignorados += 1;
        return Ok(());
    }
    // A raiz e todas as pastas acima dela: é aqui que um `0644` dentro de um `/root` fechado é
    // barrado.
    permissao::conferir(&plano.leitor, raiz, &dados)?;
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
            permissao::conferir_entrada(&plano.leitor, &caminho, &dados)?;
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

/// Se o caminho é absoluto e desta máquina.
///
/// No Windows o serviço roda como SYSTEM, e um caminho de rede (`\\servidor\pasta`) faz o
/// sistema se autenticar naquele servidor **com a conta da máquina** só por perguntar se o
/// arquivo existe — quem pediu escolheria o servidor. Só entram as unidades locais (`C:\...`,
/// também na forma `\\?\C:\...`). Caminho relativo também não: ele seria relativo à pasta de
/// trabalho do serviço, e não à de quem pediu.
pub(crate) fn caminho_local(caminho: &Path) -> bool {
    if !caminho.is_absolute() {
        return false;
    }
    #[cfg(windows)]
    {
        use std::path::{Component, Prefix};
        matches!(
            caminho.components().next(),
            Some(Component::Prefix(prefixo))
                if matches!(prefixo.kind(), Prefix::Disk(_) | Prefix::VerbatimDisk(_))
        )
    }
    #[cfg(not(windows))]
    {
        true
    }
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

#[cfg(test)]
mod testes;
