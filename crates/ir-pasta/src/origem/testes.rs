use super::*;
use crate::acao::Acao;
use ir_proto::message::{OpResult, Refusal};

const PASTA: FolderId = FolderId([1; 16]);

fn contexto() -> Contexto<'static> {
    Contexto {
        maquina_local: "DESKTOP",
        maquina_do_par: "NOTEBOOK",
        fuso_s: 0,
        diferenca_ns: 0,
        agora_ns: 1_790_962_200_000_000_000,
    }
}

fn com(arquivos: &[(&str, u8, i64)]) -> Origem {
    let mut origem = Origem::nova(PASTA);
    let retrato: Retrato = arquivos
        .iter()
        .map(|(c, r, t)| ((*c).to_owned(), Visto::arquivo(1, *t, Some([*r; 32]))))
        .collect();
    origem.aplicar_retrato(&retrato);
    origem
}

fn envio(caminho: &str, base: u64, resumo: u8, quando: i64) -> EnvioRecebido {
    EnvioRecebido {
        caminho: caminho.to_owned(),
        base,
        resumo: [resumo; 32],
        tamanho: 1,
        modificado_ns: quando,
    }
}

#[test]
fn cada_mudanca_da_varredura_ganha_o_proximo_numero() {
    let mut origem = com(&[("a", 1, 10), ("b", 2, 10)]);
    assert_eq!(origem.seq(), 2);
    let mut retrato: Retrato = Retrato::new();
    retrato.insert("a".into(), Visto::arquivo(1, 11, Some([9; 32])));
    let mudou = origem.aplicar_retrato(&retrato);
    let versoes: Vec<(String, u64, bool)> = mudou
        .iter()
        .map(|e| (e.path.clone(), e.version, e.deleted))
        .collect();
    assert_eq!(versoes, vec![("a".into(), 3, false), ("b".into(), 4, true)]);
    assert_eq!(origem.mudancas_desde(2).len(), 2);
    assert_eq!(
        origem.entrada("a").map(|e| e.id),
        Some(EntryId(1)),
        "o id fica"
    );
}

#[test]
fn o_mesmo_conteudo_dos_dois_lados_nao_e_conflito() {
    let mut origem = com(&[("a", 7, 10)]);
    let desfecho = origem.decidir_envio(&envio("a", 0, 7, 99), &contexto());
    assert_eq!(desfecho.resultado, OpResult::Accepted { version: 1 });
    assert!(desfecho.acoes.is_empty());
}

#[test]
fn base_atual_e_aplicada() {
    let mut origem = com(&[("a", 1, 10)]);
    let desfecho = origem.decidir_envio(&envio("a", 1, 2, 20), &contexto());
    assert_eq!(desfecho.resultado, OpResult::Accepted { version: 2 });
    assert_eq!(desfecho.acoes, vec![Acao::Publicar("a".into())]);
}

#[test]
fn base_antiga_e_conflito_e_a_mais_recente_fica_com_o_nome() {
    let mut origem = com(&[("a.txt", 1, 10)]);
    let desfecho = origem.decidir_envio(&envio("a.txt", 0, 2, 20), &contexto());
    let OpResult::Conflict { conflict_path, .. } = &desfecho.resultado else {
        panic!("esperava conflito: {desfecho:?}");
    };
    assert_eq!(conflict_path, "a (conflito DESKTOP 2026-10-02 17h30).txt");
    assert_eq!(
        desfecho.acoes,
        vec![
            Acao::Mover {
                de: "a.txt".into(),
                para: conflict_path.clone()
            },
            Acao::Publicar("a.txt".into())
        ]
    );
    assert_eq!(origem.entrada("a.txt").and_then(|e| e.hash), Some([2; 32]));
    assert_eq!(
        origem.entrada(conflict_path).and_then(|e| e.hash),
        Some([1; 32])
    );
}

#[test]
fn a_subpasta_com_novidade_dentro_nao_e_apagada() {
    let mut origem = Origem::nova(PASTA);
    let mut retrato = Retrato::new();
    retrato.insert("d".into(), Visto::pasta(1));
    retrato.insert("d/novo.txt".into(), Visto::arquivo(1, 1, Some([3; 32])));
    origem.aplicar_retrato(&retrato);
    let base_da_pasta = origem.entrada("d").map_or(0, |e| e.version);
    let desfecho = origem.decidir_apagar("d", base_da_pasta);
    assert!(matches!(desfecho.resultado, OpResult::Resurrected { .. }));
    assert!(origem.entrada("d/novo.txt").is_some());
}

#[test]
fn o_que_nao_viaja_e_recusado_mesmo_vindo_da_replica() {
    let mut origem = Origem::nova(PASTA);
    let desfecho = origem.decidir_envio(&envio("~$a.docx", 0, 1, 1), &contexto());
    assert_eq!(desfecho.resultado, OpResult::Refused(Refusal::UnsafePath));
}

#[test]
fn um_arquivo_novo_traz_as_pastas_de_cima() {
    let mut origem = Origem::nova(PASTA);
    let desfecho = origem.decidir_envio(&envio("x/y/z.txt", 0, 1, 1), &contexto());
    let caminhos: Vec<&str> = desfecho.mudancas.iter().map(|e| e.path.as_str()).collect();
    assert_eq!(caminhos, vec!["x", "x/y", "x/y/z.txt"]);
}
