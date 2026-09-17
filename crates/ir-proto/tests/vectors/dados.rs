//! Vetores gravados dos canais 4 e 5 — clipboard e dados.
//!
//! Em arquivo próprio pelo limite de 400 linhas por arquivo de
//! `docs/09-padroes-de-codigo.md` §1, e porque estes dois canais são os únicos cujo conteúdo
//! é de tamanho variável: um erro de posição aqui não corrompe uma tecla, corrompe um
//! arquivo inteiro do usuário.
//!
//! Duas escolhas deliberadas nos valores:
//!
//! 1. **Nenhum campo é zero** onde zero seja o padrão. Zero esconde byte perdido — some do
//!    `postcard` como varint de um byte e continua decodificando.
//! 2. **Nenhum enum de motivo usa a primeira variante.** Se alguém reordenar
//!    `DeclineReason`, `RejectReason` ou `CancelReason`, a primeira variante continua
//!    codificando `00` e o vetor passa. Usando a terceira, a troca aparece.

#![allow(unreachable_pub)]

use ir_proto::frame::{Frame, Sequence};
use ir_proto::message::{
    BulkMessage, CancelReason, ClipId, ClipKind, ClipboardMessage, DeclineReason, ManifestItem,
    Message, RejectReason, TransferId,
};

use crate::table::{Vector, v};

/// A oferta usada em todos os vetores de clipboard.
const CLIP: ClipId = ClipId(0x0004_0302);

/// A transferência usada em todos os vetores de dados.
const TRANSFER: TransferId = TransferId(0x0007_0605);

/// O resumo BLAKE3 de referência.
///
/// Não são zeros nem uma sequência crescente: zeros escondem byte perdido, e `0,1,2,3…`
/// esconde troca de duas posições vizinhas.
fn hash() -> [u8; 32] {
    let mut out = [0u8; 32];
    for (indice, slot) in out.iter_mut().enumerate() {
        *slot = u8::try_from(indice)
            .unwrap_or(0)
            .wrapping_mul(7)
            .wrapping_add(0x5a);
    }
    out
}

fn clipboard(message: ClipboardMessage, seq: u32) -> Frame {
    Frame::new(Message::Clipboard(message), Sequence(seq))
}

fn bulk(message: BulkMessage, seq: u32) -> Frame {
    Frame::new(Message::Bulk(message), Sequence(seq))
}

/// Um item de arquivo do manifesto.
fn file(path: &str, size: u64) -> ManifestItem {
    ManifestItem {
        path: path.to_owned(),
        size,
        is_dir: false,
    }
}

/// Os três itens do manifesto de referência: um diretório e dois arquivos dentro dele.
///
/// O caminho com acento existe de propósito: o campo é UTF-8 no fio e precisa sobreviver a
/// isso, porque nome de arquivo com acento é o caso comum e não a exceção.
fn items() -> Vec<ManifestItem> {
    vec![
        ManifestItem {
            path: "relatório".to_owned(),
            size: 0,
            is_dir: true,
        },
        file("relatório/janeiro.pdf", 0x1234),
        file("relatório/anexo com espaço.bin", 0x56),
    ]
}

/// Canal 4 — clipboard de texto. Todas as cinco variantes.
pub fn clipboard_vectors() -> Vec<Vector> {
    vec![
        v(
            "clip_offer",
            clipboard(
                ClipboardMessage::Offer {
                    id: CLIP,
                    kind: ClipKind::Files,
                    size: 0x0009_8765,
                    hash: hash(),
                },
                18,
            ),
            "040082861002e58e265a61686f767d848b9299a0a7aeb5bcc3cad1d8dfe6edf4fb020910171e252c33120000",
        ),
        v(
            "clip_request",
            clipboard(ClipboardMessage::Request { id: CLIP }, 19),
            "0401828610130000",
        ),
        v(
            "clip_chunk",
            clipboard(
                ClipboardMessage::Chunk {
                    id: CLIP,
                    index: 0x0123,
                    data: vec![0xde, 0xad, 0xbe, 0xef],
                },
                20,
            ),
            "0402828610a30204deadbeef140000",
        ),
        v(
            "clip_done",
            clipboard(ClipboardMessage::Done { id: CLIP }, 21),
            "0403828610150000",
        ),
        v(
            "clip_decline",
            clipboard(
                ClipboardMessage::Decline {
                    id: CLIP,
                    reason: DeclineReason::Superseded,
                },
                22,
            ),
            "040482861002160000",
        ),
    ]
}

/// Canal 5 — dados. O manifesto e a aceitação.
pub fn bulk_opening_vectors() -> Vec<Vector> {
    vec![
        v(
            "bulk_manifest",
            bulk(
                BulkMessage::Manifest {
                    id: TRANSFER,
                    items: items(),
                    total_bytes: 0x1234 + 0x56,
                },
                23,
            ),
            "0500858c1c030a72656c6174c3b372696f00011672656c6174c3b372696f2f6a616e6569726f2e706466b424002072656c6174c3b372696f2f616e65786f20636f6d2065737061c3a76f2e62696e56008a25170000",
        ),
        v(
            "bulk_accept",
            bulk(BulkMessage::Accept { id: TRANSFER }, 24),
            "0501858c1c180000",
        ),
        v(
            "bulk_reject",
            bulk(
                BulkMessage::Reject {
                    id: TRANSFER,
                    reason: RejectReason::UnsafePath,
                },
                25,
            ),
            "0502858c1c02190000",
        ),
    ]
}

/// Canal 5 — dados. O corpo da transferência: o que carrega os bytes do arquivo.
pub fn bulk_body_vectors() -> Vec<Vector> {
    vec![
        v(
            "bulk_file_start",
            bulk(
                BulkMessage::FileStart {
                    id: TRANSFER,
                    item: 2,
                },
                26,
            ),
            "0503858c1c021a0000",
        ),
        v(
            "bulk_file_block",
            // Bloco curto de propósito: o vetor grava bytes, e sessenta mil deles em
            // hexadecimal não seriam legíveis nem revisáveis. O bloco cheio tem teste
            // próprio, `a_full_file_block_fits_a_tcp_frame`.
            bulk(
                BulkMessage::FileBlock {
                    id: TRANSFER,
                    item: 2,
                    offset: 0x0001_0000,
                    data: vec![0x01, 0x02, 0x03, 0xff],
                },
                27,
            ),
            "0504858c1c0280800404010203ff1b0000",
        ),
        v(
            "bulk_file_end",
            bulk(
                BulkMessage::FileEnd {
                    id: TRANSFER,
                    item: 2,
                    hash: hash(),
                },
                28,
            ),
            "0505858c1c025a61686f767d848b9299a0a7aeb5bcc3cad1d8dfe6edf4fb020910171e252c331c0000",
        ),
    ]
}

/// Canal 5 — dados. O que volta do destino, e o cancelamento.
pub fn bulk_closing_vectors() -> Vec<Vector> {
    vec![
        v(
            "bulk_verified",
            bulk(
                BulkMessage::Verified {
                    id: TRANSFER,
                    item: 2,
                    ok: true,
                },
                29,
            ),
            "0506858c1c02011d0000",
        ),
        v(
            "bulk_progress",
            bulk(
                BulkMessage::Progress {
                    id: TRANSFER,
                    bytes_done: 0x00ab_cdef,
                },
                30,
            ),
            "0507858c1cef9baf051e0000",
        ),
        v(
            "bulk_cancel",
            bulk(
                BulkMessage::Cancel {
                    id: TRANSFER,
                    reason: CancelReason::WriteFailed,
                },
                31,
            ),
            "0508858c1c021f0000",
        ),
    ]
}
