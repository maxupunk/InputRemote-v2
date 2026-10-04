//! Vetores gravados da pasta compartilhada — versão 8, `BulkMessage::Folder`.
//!
//! Em arquivo próprio pelo limite de 400 linhas, e porque a pasta tem o próprio enum: o terceiro
//! byte de cada vetor é o número da variante de `FolderMessage`, e reordenar aquele enum troca
//! uma operação por outra na máquina do usuário — um "apagar" lido como "criar subpasta".
//!
//! As mesmas duas escolhas de `dados.rs`: nenhum campo é zero onde zero seja o padrão, e nenhum
//! enum de motivo usa a primeira variante.

#![allow(unreachable_pub)]

use ir_proto::frame::{Frame, Sequence};
use ir_proto::message::{
    BulkMessage, DeclineFolder, Entry, EntryId, EntryKind, FolderId, FolderMessage, KnownFolder,
    Message, OpId, OpResult, RangeFailure, RangeId, Role,
};

use crate::table::{Vector, v};

/// A pasta de todos os vetores. Bytes diferentes entre si, para uma troca de posição aparecer.
const PASTA: FolderId = FolderId([
    0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99, 0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xf1, 0xf2,
]);

const OP: OpId = OpId(0x0003_0201);
const PEDIDO: RangeId = RangeId(0x0006_0504);

/// Um resumo sem zeros e sem sequência crescente, como o de `dados.rs`.
fn resumo() -> [u8; 32] {
    let mut out = [0u8; 32];
    for (indice, slot) in out.iter_mut().enumerate() {
        *slot = u8::try_from(indice)
            .unwrap_or(0)
            .wrapping_mul(11)
            .wrapping_add(0x3c);
    }
    out
}

fn pasta(message: FolderMessage, seq: u32) -> Frame {
    Frame::new(Message::Bulk(BulkMessage::Folder(message)), Sequence(seq))
}

/// Uma subpasta e um arquivo dentro dela, com acento, na ordem de versão.
fn entradas() -> Vec<Entry> {
    vec![
        Entry {
            id: EntryId(0x0102),
            path: "relatório".to_owned(),
            kind: EntryKind::Directory,
            size: 0,
            modified_ns: 1_790_000_000_123_456_789,
            hash: None,
            version: 0x41,
            deleted: false,
        },
        Entry {
            id: EntryId(0x0304),
            path: "relatório/março.xlsx".to_owned(),
            kind: EntryKind::File,
            size: 0x0002_3456,
            modified_ns: 1_790_000_000_987_654_321,
            hash: Some(resumo()),
            version: 0x42,
            deleted: true,
        },
    ]
}

/// Sessão e ciclo de vida.
pub fn pasta_abertura_vectors() -> Vec<Vector> {
    vec![
        v(
            "folder_hello",
            pasta(
                FolderMessage::Hello {
                    folders: vec![KnownFolder {
                        folder: PASTA,
                        role: Role::Replica,
                        seen_up_to: 0x0123,
                    }],
                    clock_ns: 1_790_000_000_000_000_007,
                    reply: true,
                },
                40,
            ),
            "050a0001112233445566778899aabbccddeef1f201a3028e8098bf84e1add73101280000",
        ),
        v(
            "folder_helper_absent",
            pasta(FolderMessage::HelperAbsent, 41),
            "050a01290000",
        ),
        v(
            "folder_offer",
            pasta(
                FolderMessage::Offer {
                    folder: PASTA,
                    name: "Projetos é".to_owned(),
                    entries: 0x0789,
                    total_bytes: 0x0001_2345_6789,
                },
                42,
            ),
            "050a02112233445566778899aabbccddeef1f20b50726f6a65746f7320c3a9890f89cf959a122a0000",
        ),
        v(
            "folder_accept",
            pasta(FolderMessage::Accept { folder: PASTA }, 43),
            "050a03112233445566778899aabbccddeef1f22b0000",
        ),
        v(
            "folder_decline",
            pasta(
                FolderMessage::Decline {
                    folder: PASTA,
                    reason: DeclineFolder::TooManyFolders,
                },
                44,
            ),
            "050a04112233445566778899aabbccddeef1f2022c0000",
        ),
        v(
            "folder_stop",
            pasta(FolderMessage::Stop { folder: PASTA }, 45),
            "050a05112233445566778899aabbccddeef1f22d0000",
        ),
    ]
}

/// O índice: pedir, mandar e confirmar mudanças.
pub fn pasta_indice_vectors() -> Vec<Vector> {
    vec![
        v(
            "folder_request_changes",
            pasta(
                FolderMessage::RequestChanges {
                    folder: PASTA,
                    since: 0x0abc,
                },
                46,
            ),
            "050a06112233445566778899aabbccddeef1f2bc152e0000",
        ),
        v(
            "folder_changes",
            pasta(
                FolderMessage::Changes {
                    folder: PASTA,
                    entries: entradas(),
                    up_to: 0x43,
                    last: true,
                },
                47,
            ),
            "050a07112233445566778899aabbccddeef1f20282020a72656c6174c3b372696f0100aab4f6b485e1add73100410084061672656c6174c3b372696f2f6d6172c3a76f2e786c737800d6e808e2a28bed8be1add731013c47525d68737e89949faab5c0cbd6e1ecf7020d18232e39444f5a65707b8691420143012f0000",
        ),
        v(
            "folder_acknowledge",
            pasta(
                FolderMessage::Acknowledge {
                    folder: PASTA,
                    seq: 0x0def,
                },
                48,
            ),
            "050a08112233445566778899aabbccddeef1f2ef1b300000",
        ),
    ]
}

/// Trechos baixados da origem.
pub fn pasta_conteudo_vectors() -> Vec<Vector> {
    vec![
        v(
            "folder_request_range",
            pasta(
                FolderMessage::RequestRange {
                    folder: PASTA,
                    request: PEDIDO,
                    entry: EntryId(0x0304),
                    version: 0x42,
                    offset: 0x0001_0000,
                    len: 0x0004_0000,
                },
                49,
            ),
            "050a09112233445566778899aabbccddeef1f2848a18840642808004808010310000",
        ),
        v(
            "folder_range",
            pasta(
                FolderMessage::Range {
                    folder: PASTA,
                    request: PEDIDO,
                    offset: 0x0001_f000,
                    data: vec![0xca, 0xfe, 0x01],
                },
                50,
            ),
            "050a0a112233445566778899aabbccddeef1f2848a1880e00703cafe01320000",
        ),
        v(
            "folder_range_failed",
            pasta(
                FolderMessage::RangeFailed {
                    folder: PASTA,
                    request: PEDIDO,
                    reason: RangeFailure::Unreadable,
                },
                51,
            ),
            "050a0b112233445566778899aabbccddeef1f2848a1802330000",
        ),
        v(
            "folder_cancel_range",
            pasta(
                FolderMessage::CancelRange {
                    folder: PASTA,
                    request: PEDIDO,
                },
                52,
            ),
            "050a0c112233445566778899aabbccddeef1f2848a18340000",
        ),
    ]
}

/// Envios da réplica para a origem.
pub fn pasta_envio_vectors() -> Vec<Vector> {
    vec![
        v(
            "folder_upload",
            pasta(
                FolderMessage::Upload {
                    folder: PASTA,
                    op: OP,
                    path: "relatório/março.xlsx".to_owned(),
                    base: 0x42,
                    size: 0x0002_3457,
                    hash: resumo(),
                    modified_ns: 1_790_000_100_000_000_003,
                },
                53,
            ),
            "050a0d112233445566778899aabbccddeef1f281840c1672656c6174c3b372696f2f6d6172c3a76f2e786c737842d7e8083c47525d68737e89949faab5c0cbd6e1ecf7020d18232e39444f5a65707b869186a0cfc6ede6add731350000",
        ),
        v(
            "folder_credit",
            pasta(
                FolderMessage::Credit {
                    folder: PASTA,
                    op: OP,
                    bytes: 0x0040_0000,
                },
                54,
            ),
            "050a0e112233445566778899aabbccddeef1f281840c80808002360000",
        ),
        v(
            "folder_already_have",
            pasta(
                FolderMessage::AlreadyHave {
                    folder: PASTA,
                    op: OP,
                },
                55,
            ),
            "050a0f112233445566778899aabbccddeef1f281840c370000",
        ),
        v(
            "folder_upload_block",
            pasta(
                FolderMessage::UploadBlock {
                    folder: PASTA,
                    op: OP,
                    offset: 0x0002_0000,
                    data: vec![0xbe, 0xef, 0x02],
                },
                56,
            ),
            "050a10112233445566778899aabbccddeef1f281840c80800803beef02380000",
        ),
    ]
}

/// O fim de um envio e as outras operações da réplica.
pub fn pasta_operacao_vectors() -> Vec<Vector> {
    vec![
        v(
            "folder_upload_end",
            pasta(
                FolderMessage::UploadEnd {
                    folder: PASTA,
                    op: OP,
                },
                57,
            ),
            "050a11112233445566778899aabbccddeef1f281840c390000",
        ),
        v(
            "folder_delete",
            pasta(
                FolderMessage::Delete {
                    folder: PASTA,
                    op: OP,
                    path: "velho.txt".to_owned(),
                    base: 0x17,
                },
                58,
            ),
            "050a12112233445566778899aabbccddeef1f281840c0976656c686f2e747874173a0000",
        ),
        v(
            "folder_create_directory",
            pasta(
                FolderMessage::CreateDirectory {
                    folder: PASTA,
                    op: OP,
                    path: "novos".to_owned(),
                },
                59,
            ),
            "050a13112233445566778899aabbccddeef1f281840c056e6f766f733b0000",
        ),
        v(
            "folder_rename",
            pasta(
                FolderMessage::Rename {
                    folder: PASTA,
                    op: OP,
                    from: "a.txt".to_owned(),
                    to: "novos/b.txt".to_owned(),
                    base: 0x18,
                },
                60,
            ),
            "050a14112233445566778899aabbccddeef1f281840c05612e7478740b6e6f766f732f622e747874183c0000",
        ),
    ]
}

/// O desfecho que a origem devolve.
pub fn pasta_desfecho_vectors() -> Vec<Vector> {
    vec![
        v(
            "folder_outcome",
            pasta(
                FolderMessage::Outcome {
                    folder: PASTA,
                    op: OP,
                    result: OpResult::Conflict {
                        version: 0x44,
                        conflict_path: "relatório/março (conflito NOTEBOOK).xlsx".to_owned(),
                    },
                },
                61,
            ),
            "050a15112233445566778899aabbccddeef1f281840c01442a72656c6174c3b372696f2f6d6172c3a76f2028636f6e666c69746f204e4f5445424f4f4b292e786c73783d0000",
        ),
        v(
            "folder_copied",
            pasta(
                FolderMessage::Copied {
                    folder: PASTA,
                    paths: vec!["fotos/img2.jpg".to_owned(), "notas.txt".to_owned()],
                },
                62,
            ),
            "050a16112233445566778899aabbccddeef1f2020e666f746f732f696d67322e6a7067096e6f7461732e7478743e0000",
        ),
    ]
}
