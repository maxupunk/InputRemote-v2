//! O manifesto em quantos quadros ele precisar.
//!
//! Um quadro TCP leva no máximo [`limits::MAX_TCP_PLAINTEXT`] bytes, e um manifesto pode ter até
//! [`limits::MAX_MANIFEST_ITEMS`] itens com até [`limits::MAX_RELATIVE_PATH`] bytes de caminho —
//! uns dez megabytes. Num quadro só, uma pasta com mil e poucos arquivos já não cabia: o codec
//! recusava o manifesto, quem enviava tomava a recusa por queda do enlace, e a tela dizia "a
//! conexão de arquivos caiu". Visto numa pasta de jogo copiada do Linux para o Windows
//! ([log 55](../../../../../docs/logs/55-o-manifesto-que-nao-cabia.md)).
//!
//! A regra fica aqui, no crate puro, porque é de formato: quanto cabe num quadro só se sabe pelo
//! tamanho que o `postcard` dá a cada item.

use super::{BulkMessage, ManifestItem, TransferId};
use crate::limits;

/// Quanto do quadro fica para o que não é item: canal, variante, identificador, contagem, total,
/// sequência, confirmação carregada e encarnação. O pior caso fica abaixo de 64 bytes; o dobro é
/// folga que não custa nada — no máximo um quadro a mais num manifesto de megabytes.
const ENVELOPE: usize = 128;

/// Quantos bytes de itens cabem num quadro.
const ORCAMENTO: usize = limits::MAX_TCP_PLAINTEXT - ENVELOPE;

/// As mensagens que levam este manifesto: as [`BulkMessage::ManifestPart`] que ele precisar e o
/// [`BulkMessage::Manifest`] por último, com a parte final e o total.
///
/// Um manifesto que cabe num quadro sai como uma mensagem só, idêntica à de antes das partes — e
/// é isso que mantém a cópia comum funcionando com um par que ainda não as conhece.
#[must_use]
pub fn manifest_messages(
    id: TransferId,
    items: Vec<ManifestItem>,
    total_bytes: u64,
) -> Vec<BulkMessage> {
    let mut mensagens = Vec::new();
    let mut atual = Vec::new();
    let mut ocupado = 0usize;
    for item in items {
        let custo = encoded_len(&item);
        if !atual.is_empty() && ocupado.saturating_add(custo) > ORCAMENTO {
            let items = std::mem::take(&mut atual);
            mensagens.push(BulkMessage::ManifestPart { id, items });
            ocupado = 0;
        }
        ocupado = ocupado.saturating_add(custo);
        atual.push(item);
    }
    mensagens.push(BulkMessage::Manifest {
        id,
        items: atual,
        total_bytes,
    });
    mensagens
}

/// O tamanho exato de um item no `postcard`: o caminho com o prefixo de comprimento, o tamanho em
/// *varint* e o byte do `bool`.
fn encoded_len(item: &ManifestItem) -> usize {
    let comprimento = u64::try_from(item.path.len()).unwrap_or(u64::MAX);
    varint_len(comprimento) + item.path.len() + varint_len(item.size) + 1
}

/// Quantos bytes um inteiro ocupa em *varint*: sete bits por byte, e ao menos um.
const fn varint_len(valor: u64) -> usize {
    let bits = (u64::BITS - valor.leading_zeros()) as usize;
    if bits == 0 { 1 } else { bits.div_ceil(7) }
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use crate::carrier::Carrier;
    use crate::channel::ChannelId;
    use crate::codec;
    use crate::frame::{Ack, ChannelAck, Epoch, Frame, Sequence};
    use crate::message::Message;

    fn item(path: String, size: u64) -> ManifestItem {
        ManifestItem {
            path,
            size,
            is_dir: false,
        }
    }

    /// O quadro mais gordo que a mensagem pode ganhar: tudo o que é do envelope no máximo.
    fn quadro_no_pior_caso(mensagem: BulkMessage) -> Frame {
        let mut quadro = Frame::new(Message::Bulk(mensagem), Sequence(u32::MAX));
        quadro.ack = Some(ChannelAck::new(
            ChannelId::Bulk,
            Ack {
                cumulative: Sequence(u32::MAX),
                bits: u32::MAX,
            },
        ));
        quadro.epoch = Epoch(u32::MAX);
        quadro
    }

    /// Os itens de volta, na ordem, a partir das mensagens — o que quem recebe faz.
    fn remontar(mensagens: &[BulkMessage]) -> (Vec<ManifestItem>, u64) {
        let mut itens = Vec::new();
        for (posicao, mensagem) in mensagens.iter().enumerate() {
            let ultima = posicao + 1 == mensagens.len();
            match mensagem {
                BulkMessage::ManifestPart { items, .. } if !ultima => itens.extend(items.clone()),
                BulkMessage::Manifest {
                    items, total_bytes, ..
                } if ultima => {
                    itens.extend(items.clone());
                    return (itens, *total_bytes);
                }
                outra => panic!("fora de ordem na posição {posicao}: {outra:?}"),
            }
        }
        panic!("sem o Manifest no fim");
    }

    #[test]
    fn um_manifesto_pequeno_sai_como_antes_numa_mensagem_so() {
        let itens = vec![item("a.txt".to_owned(), 3), item("b.txt".to_owned(), 4)];
        let mensagens = manifest_messages(TransferId(1), itens.clone(), 7);
        assert_eq!(
            mensagens,
            vec![BulkMessage::Manifest {
                id: TransferId(1),
                items: itens,
                total_bytes: 7,
            }]
        );
    }

    #[test]
    fn uma_pasta_de_jogo_nao_cabia_num_quadro_e_agora_vai_em_partes() {
        // O defeito: três mil arquivos de nome comum passam de 64 KiB num quadro só.
        let itens: Vec<ManifestItem> = (0..3_000)
            .map(|n| {
                item(
                    format!("Jogo eletronica 2/Data/Textures/t{n:05}.png"),
                    40_000,
                )
            })
            .collect();
        let inteiro = BulkMessage::Manifest {
            id: TransferId(9),
            items: itens.clone(),
            total_bytes: 120_000_000,
        };
        assert!(
            codec::encode(&quadro_no_pior_caso(inteiro), Carrier::Tcp).is_err(),
            "o cenário do defeito: inteiro, não cabe"
        );

        let mensagens = manifest_messages(TransferId(9), itens.clone(), 120_000_000);
        assert!(mensagens.len() > 1, "precisou de partes");
        for mensagem in &mensagens {
            codec::encode(&quadro_no_pior_caso(mensagem.clone()), Carrier::Tcp)
                .expect("cada parte cabe num quadro");
        }
        assert_eq!(remontar(&mensagens), (itens, 120_000_000));
    }

    #[test]
    fn o_maior_manifesto_que_o_protocolo_aceita_cabe_em_quadros() {
        // O pior caso inteiro: o máximo de itens, cada um com o caminho mais longo e o maior
        // tamanho. Se o envelope reservado fosse curto, algum quadro passaria do teto aqui.
        let caminho = "x".repeat(limits::MAX_RELATIVE_PATH);
        let itens: Vec<ManifestItem> = (0..limits::MAX_MANIFEST_ITEMS)
            .map(|_| item(caminho.clone(), u64::MAX))
            .collect();
        let mensagens = manifest_messages(TransferId(u32::MAX), itens.clone(), u64::MAX);
        for mensagem in &mensagens {
            codec::encode(&quadro_no_pior_caso(mensagem.clone()), Carrier::Tcp)
                .expect("cada parte cabe num quadro");
        }
        assert_eq!(remontar(&mensagens).0.len(), itens.len());
    }

    #[test]
    fn o_tamanho_calculado_e_o_que_o_postcard_produz() {
        let casos = [
            item(String::new(), 0),
            item("a".to_owned(), 127),
            item("acentuação/relatório.pdf".to_owned(), 128),
            item("y".repeat(200), 16_384),
            item("z".repeat(limits::MAX_RELATIVE_PATH), u64::MAX),
        ];
        for caso in casos {
            let bytes = postcard::to_allocvec(&caso).expect("codifica");
            assert_eq!(encoded_len(&caso), bytes.len(), "{caso:?}");
        }
    }
}
