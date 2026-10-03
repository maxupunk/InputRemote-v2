use super::*;
use crate::retrato::Retrato;

const PASTA: FolderId = FolderId([2; 16]);

fn retrato(itens: &[(&str, Visto)]) -> Retrato {
    itens.iter().map(|(c, v)| ((*c).to_owned(), *v)).collect()
}

fn arquivo(t: i64) -> Visto {
    Visto::arquivo(1, t, Some([t.to_le_bytes()[0]; 32]))
}

fn caminhos_da_fila(replica: &mut Replica) -> Vec<String> {
    let mut saida = Vec::new();
    while let Some((op, operacao)) = replica.proxima() {
        let caminho = match operacao {
            Operacao::Enviar { caminho, .. } => format!("enviar {caminho}"),
            Operacao::Apagar { caminho, .. } => format!("apagar {caminho}"),
            Operacao::CriarPasta { caminho } => format!("pasta {caminho}"),
        };
        saida.push(caminho);
        replica.resultado(op, &OpResult::Accepted { version: 9 }, None);
    }
    saida
}

#[test]
fn a_fila_cria_de_cima_para_baixo_e_apaga_de_baixo_para_cima() {
    let mut replica = Replica::nova(PASTA, false);
    replica.aplicar_retrato(&retrato(&[
        ("d", Visto::pasta(1)),
        ("d/e", Visto::pasta(1)),
        ("d/e/f.txt", arquivo(1)),
    ]));
    assert_eq!(
        caminhos_da_fila(&mut replica),
        vec!["pasta d", "pasta d/e", "enviar d/e/f.txt"]
    );
    replica.aplicar_retrato(&Retrato::new());
    assert_eq!(
        caminhos_da_fila(&mut replica),
        vec!["apagar d/e/f.txt", "apagar d/e", "apagar d"]
    );
}

#[test]
fn uma_operacao_por_vez_e_a_queda_devolve_a_que_estava_no_ar() {
    let mut replica = Replica::nova(PASTA, false);
    replica.aplicar_retrato(&retrato(&[("a", arquivo(1)), ("b", arquivo(2))]));
    let (primeira, _) = replica.proxima().unwrap();
    assert!(replica.proxima().is_none(), "espera a resposta da primeira");
    replica.canal_caiu();
    let (de_novo, operacao) = replica.proxima().unwrap();
    assert_ne!(primeira, de_novo, "sai com número novo");
    assert_eq!(
        operacao,
        Operacao::Enviar {
            caminho: "a".into(),
            base: 0
        }
    );
}

#[test]
fn o_caminho_sujo_nao_e_sobrescrito_pelo_que_vem_da_origem() {
    let mut replica = Replica::nova(PASTA, false);
    replica.aplicar_retrato(&retrato(&[("a", arquivo(1))]));
    let entrada = Entry {
        id: ir_proto::message::EntryId(1),
        path: "a".into(),
        kind: EntryKind::File,
        size: 1,
        modified_ns: 5,
        hash: Some([77; 32]),
        version: 3,
        deleted: false,
    };
    let acoes = replica.aplicar_mudancas(&[entrada], 3);
    assert!(acoes.is_empty(), "{acoes:?}");
    assert_eq!(replica.visto_ate(), 3);
    assert!(!replica.pode_publicar("a", Some(&arquivo(1))));
}

#[test]
fn travado_do_outro_lado_volta_para_o_fim_da_fila() {
    let mut replica = Replica::nova(PASTA, false);
    replica.aplicar_retrato(&retrato(&[("a", arquivo(1)), ("b", arquivo(2))]));
    let (op, _) = replica.proxima().unwrap();
    replica.resultado(op, &OpResult::Refused(Refusal::Locked), None);
    assert_eq!(caminhos_da_fila(&mut replica), vec!["enviar b", "enviar a"]);
    assert_eq!(replica.recusadas(), 0);
}
