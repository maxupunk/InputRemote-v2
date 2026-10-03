//! As mudanças do índice em quantos quadros elas precisarem.
//!
//! A primeira sincronia de uma pasta manda o índice inteiro — até
//! [`limits::MAX_FOLDER_ENTRIES`] entradas —, e um quadro TCP leva no máximo
//! [`limits::MAX_TCP_PLAINTEXT`] bytes. É o mesmo problema do manifesto que não cabia
//! ([log 55](../../../../../../docs/logs/55-o-manifesto-que-nao-cabia.md)), resolvido do mesmo jeito:
//! quanto cabe se sabe pelo tamanho que o `postcard` dá a cada entrada.

use super::{Entry, FolderId, FolderMessage};
use crate::limits;

/// Quanto do quadro fica para o que não é entrada: canal, as três variantes, a pasta, a contagem,
/// `up_to`, `last`, sequência, confirmação carregada e encarnação. O pior caso fica abaixo de 80
/// bytes; o resto é folga que custa no máximo um quadro a mais.
const ENVELOPE: usize = 160;

/// Quantos bytes de entradas cabem num quadro.
const ORCAMENTO: usize = limits::MAX_TCP_PLAINTEXT - ENVELOPE;

/// As mensagens [`FolderMessage::Changes`] que levam estas entradas, a última com `last`.
///
/// As entradas saem em ordem de versão, e cada mensagem diz em `up_to` até onde a réplica fica
/// completa depois de aplicá-la. Várias entradas podem ter a mesma versão — um conflito grava duas
/// de uma vez — e uma versão pode ser partida entre duas mensagens. Por isso o `up_to` de uma
/// mensagem do meio é o número **anterior** à primeira versão da mensagem seguinte: se o enlace cair
/// entre as duas, a réplica pede de novo a versão partida inteira, e nada fica para trás.
#[must_use]
pub fn changes_messages(
    folder: FolderId,
    mut entries: Vec<Entry>,
    up_to: u64,
) -> Vec<FolderMessage> {
    entries.sort_by_key(|entrada| entrada.version);
    let mut paginas: Vec<Vec<Entry>> = Vec::new();
    let mut atual = Vec::new();
    let mut ocupado = 0usize;
    for entrada in entries {
        let custo = encoded_len(&entrada);
        if !atual.is_empty() && ocupado.saturating_add(custo) > ORCAMENTO {
            paginas.push(std::mem::take(&mut atual));
            ocupado = 0;
        }
        ocupado = ocupado.saturating_add(custo);
        atual.push(entrada);
    }
    paginas.push(atual);

    let primeiras: Vec<Option<u64>> = paginas
        .iter()
        .skip(1)
        .map(|pagina| pagina.first().map(|entrada| entrada.version))
        .collect();
    let total = paginas.len();
    paginas
        .into_iter()
        .enumerate()
        .map(|(posicao, entries)| {
            let last = posicao + 1 == total;
            let alcance = match primeiras.get(posicao).copied().flatten() {
                Some(proxima) if !last => proxima.saturating_sub(1).min(up_to),
                _ => up_to,
            };
            FolderMessage::Changes {
                folder,
                entries,
                up_to: alcance,
                last,
            }
        })
        .collect()
}

/// O tamanho de uma entrada no `postcard`.
///
/// Medido codificando, e não somando à mão como no manifesto: a entrada tem `Option`, `i64` em
/// *zigzag* e dois enums, e uma conta à mão que erra por um byte vira um quadro recusado. Uma
/// entrada que não codifica ocupa o orçamento inteiro e sai sozinha — o codec a recusa ali, em vez
/// de levar as vizinhas junto.
fn encoded_len(entry: &Entry) -> usize {
    postcard::to_allocvec(entry).map_or(ORCAMENTO, |bytes| bytes.len())
}
