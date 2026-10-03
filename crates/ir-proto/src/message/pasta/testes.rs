#![allow(clippy::panic)]

use super::*;
use crate::carrier::Carrier;
use crate::channel::ChannelId;
use crate::codec;
use crate::error::ProtoError;
use crate::frame::{Ack, ChannelAck, Epoch, Frame, Sequence};
use crate::limits;
use crate::message::{BulkMessage, Message};

const PASTA: FolderId = FolderId([0xa7; 16]);

fn entrada(path: &str, version: u64) -> Entry {
    Entry {
        id: EntryId(version),
        path: path.to_owned(),
        kind: EntryKind::File,
        size: 10,
        modified_ns: 1_790_000_000_000_000_000,
        hash: Some([3; 32]),
        version,
        deleted: false,
    }
}

/// O quadro mais gordo que a mensagem pode ganhar: tudo o que é do envelope no máximo.
fn quadro_no_pior_caso(mensagem: FolderMessage) -> Frame {
    let mut quadro = Frame::new(
        Message::Bulk(BulkMessage::Folder(mensagem)),
        Sequence(u32::MAX),
    );
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

fn cabe(mensagem: FolderMessage) -> usize {
    let quadro = quadro_no_pior_caso(mensagem);
    let bytes = codec::encode(&quadro, Carrier::Tcp).expect("tem de caber num quadro TCP");
    assert_eq!(codec::decode(&bytes, Carrier::Tcp).unwrap(), quadro);
    limits::MAX_TCP_PLAINTEXT - bytes.len()
}

#[test]
fn um_trecho_e_um_bloco_cheios_cabem_num_quadro_com_folga() {
    let trecho = FolderMessage::Range {
        folder: PASTA,
        request: RangeId(u32::MAX),
        offset: u64::MAX,
        data: vec![0xa5; limits::MAX_FILE_BLOCK],
    };
    let bloco = FolderMessage::UploadBlock {
        folder: PASTA,
        op: OpId(u32::MAX),
        offset: u64::MAX,
        data: vec![0x5a; limits::MAX_FILE_BLOCK],
    };
    for mensagem in [trecho, bloco] {
        let folga = cabe(mensagem);
        assert!(folga >= 64, "a folga caiu para {folga} B");
    }
}

#[test]
fn o_maior_envio_e_a_maior_renomeacao_cabem() {
    let longo = "x".repeat(limits::MAX_RELATIVE_PATH);
    cabe(FolderMessage::Upload {
        folder: PASTA,
        op: OpId(u32::MAX),
        path: longo.clone(),
        base: u64::MAX,
        size: u64::MAX,
        hash: [0xff; 32],
        modified_ns: i64::MIN,
    });
    cabe(FolderMessage::Rename {
        folder: PASTA,
        op: OpId(u32::MAX),
        from: longo.clone(),
        to: longo,
        base: u64::MAX,
    });
}

#[test]
fn o_hello_mais_cheio_cabe() {
    let pastas = (0..limits::MAX_KNOWN_FOLDERS)
        .map(|_| KnownFolder {
            folder: PASTA,
            role: Role::Replica,
            seen_up_to: u64::MAX,
        })
        .collect();
    cabe(FolderMessage::Hello {
        folders: pastas,
        clock_ns: i64::MAX,
        reply: true,
    });
}

#[test]
fn caminhos_inseguros_sao_recusados_em_toda_mensagem_que_grava() {
    let ruins = ["../fora.txt", "/etc/passwd", "a/C:x.dll", "nul.txt", ""];
    for ruim in ruins {
        let mensagens = [
            FolderMessage::Upload {
                folder: PASTA,
                op: OpId(1),
                path: ruim.to_owned(),
                base: 0,
                size: 1,
                hash: [0; 32],
                modified_ns: 0,
            },
            FolderMessage::Delete {
                folder: PASTA,
                op: OpId(1),
                path: ruim.to_owned(),
                base: 1,
            },
            FolderMessage::CreateDirectory {
                folder: PASTA,
                op: OpId(1),
                path: ruim.to_owned(),
            },
            FolderMessage::Rename {
                folder: PASTA,
                op: OpId(1),
                from: "ok.txt".to_owned(),
                to: ruim.to_owned(),
                base: 1,
            },
            FolderMessage::Changes {
                folder: PASTA,
                entries: vec![entrada(ruim, 1)],
                up_to: 1,
                last: true,
            },
            FolderMessage::Outcome {
                folder: PASTA,
                op: OpId(1),
                result: OpResult::Conflict {
                    version: 2,
                    conflict_path: ruim.to_owned(),
                },
            },
        ];
        for mensagem in mensagens {
            assert_eq!(
                validate_folder_message(&mensagem),
                Err(ProtoError::Malformed),
                "{ruim:?} passou em {mensagem:?}"
            );
        }
    }
}

#[test]
fn o_nome_da_pasta_e_um_componente_so() {
    let oferta = |name: &str| FolderMessage::Offer {
        folder: PASTA,
        name: name.to_owned(),
        entries: 1,
        total_bytes: 1,
    };
    assert_eq!(validate_folder_message(&oferta("Projetos 2026")), Ok(()));
    assert_eq!(
        validate_folder_message(&oferta("a/b")),
        Err(ProtoError::Malformed)
    );
    assert!(matches!(
        validate_folder_message(&oferta(&"n".repeat(limits::MAX_FOLDER_NAME + 1))),
        Err(ProtoError::TooLarge { .. })
    ));
}

#[test]
fn pedido_e_bloco_acima_do_limite_sao_recusados() {
    let pedido = |len| FolderMessage::RequestRange {
        folder: PASTA,
        request: RangeId(1),
        entry: EntryId(1),
        version: 1,
        offset: 0,
        len,
    };
    assert_eq!(
        validate_folder_message(&pedido(limits::MAX_RANGE_REQUEST)),
        Ok(())
    );
    assert!(validate_folder_message(&pedido(limits::MAX_RANGE_REQUEST + 1)).is_err());

    let bloco = FolderMessage::Range {
        folder: PASTA,
        request: RangeId(1),
        offset: 0,
        data: vec![0; limits::MAX_FILE_BLOCK + 1],
    };
    assert!(validate_folder_message(&bloco).is_err());
}

#[test]
fn o_hello_com_pastas_demais_e_recusado_antes_de_usar() {
    let pastas = (0..=limits::MAX_KNOWN_FOLDERS)
        .map(|_| KnownFolder {
            folder: PASTA,
            role: Role::Origin,
            seen_up_to: 1,
        })
        .collect();
    let hello = FolderMessage::Hello {
        folders: pastas,
        clock_ns: 0,
        reply: false,
    };
    assert!(matches!(
        validate_folder_message(&hello),
        Err(ProtoError::CountTooLarge { .. })
    ));
}

#[test]
fn mudancas_fora_de_ordem_ou_alem_do_alcance_sao_recusadas() {
    let leva = |entries, up_to, last| FolderMessage::Changes {
        folder: PASTA,
        entries,
        up_to,
        last,
    };
    let em_ordem = vec![entrada("a", 1), entrada("b", 2), entrada("c", 2)];
    assert_eq!(
        validate_folder_message(&leva(em_ordem.clone(), 2, true)),
        Ok(())
    );
    assert_eq!(
        validate_folder_message(&leva(em_ordem.clone(), 1, true)),
        Err(ProtoError::Malformed),
        "a última da leva passou do alcance"
    );
    assert_eq!(
        validate_folder_message(&leva(em_ordem, 1, false)),
        Ok(()),
        "no meio da leva, a versão partida passa do alcance"
    );
    let fora = vec![entrada("a", 2), entrada("b", 1)];
    assert_eq!(
        validate_folder_message(&leva(fora, 2, true)),
        Err(ProtoError::Malformed)
    );
    let mut subpasta = entrada("d", 1);
    subpasta.kind = EntryKind::Directory;
    assert_eq!(
        validate_folder_message(&leva(vec![subpasta], 1, true)),
        Err(ProtoError::Malformed),
        "subpasta não tem tamanho"
    );
}

#[test]
fn um_indice_pequeno_vai_numa_mensagem_so() {
    let mensagens = changes_messages(PASTA, vec![entrada("b", 2), entrada("a", 1)], 2);
    assert_eq!(
        mensagens,
        vec![FolderMessage::Changes {
            folder: PASTA,
            entries: vec![entrada("a", 1), entrada("b", 2)],
            up_to: 2,
            last: true,
        }]
    );
}

#[test]
fn um_indice_vazio_ainda_diz_ate_onde_a_replica_esta_em_dia() {
    let mensagens = changes_messages(PASTA, Vec::new(), 7);
    assert_eq!(
        mensagens,
        vec![FolderMessage::Changes {
            folder: PASTA,
            entries: Vec::new(),
            up_to: 7,
            last: true,
        }]
    );
}

#[test]
fn o_maior_indice_vai_em_quadros_que_cabem_e_chega_inteiro() {
    let caminho = "y".repeat(limits::MAX_RELATIVE_PATH);
    let total = 2_000u64;
    let entradas: Vec<Entry> = (1..=total).map(|v| entrada(&caminho, v)).collect();
    let mensagens = changes_messages(PASTA, entradas.clone(), total);
    assert!(mensagens.len() > 1, "precisou de várias mensagens");

    let mut chegou = Vec::new();
    for (posicao, mensagem) in mensagens.iter().enumerate() {
        cabe(mensagem.clone());
        assert_eq!(validate_folder_message(mensagem), Ok(()));
        let FolderMessage::Changes { entries, last, .. } = mensagem else {
            panic!("só mudanças");
        };
        assert_eq!(*last, posicao + 1 == mensagens.len());
        chegou.extend(entries.iter().cloned());
    }
    assert_eq!(chegou, entradas);
}

/// O defeito que o `up_to` do meio evita: um conflito grava duas entradas na mesma versão, e se a
/// quebra cair entre elas e o enlace cair junto, a réplica que guardasse a versão da última entrada
/// aplicada nunca receberia a segunda.
#[test]
fn uma_versao_partida_entre_mensagens_e_pedida_de_novo_inteira() {
    let caminho = "z".repeat(limits::MAX_RELATIVE_PATH);
    let entradas: Vec<Entry> = (0..400).map(|_| entrada(&caminho, 5)).collect();
    let mensagens = changes_messages(PASTA, entradas, 9);
    assert!(mensagens.len() > 1, "a versão 5 não coube num quadro");
    for mensagem in &mensagens {
        let FolderMessage::Changes { up_to, last, .. } = mensagem else {
            panic!("só mudanças");
        };
        let esperado = if *last { 9 } else { 4 };
        assert_eq!(*up_to, esperado, "a versão 5 só fica completa no fim");
    }
}
