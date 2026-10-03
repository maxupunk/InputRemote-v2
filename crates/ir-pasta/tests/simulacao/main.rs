//! A pasta compartilhada de ponta a ponta, entre dois computadores simulados.
//!
//! Cada teste é uma história que o usuário viveria: compartilhar, editar dos dois lados, ficar
//! offline, voltar. O critério é sempre o que o usuário vê — as duas árvores iguais — e o que ele
//! não pode perder — nenhuma versão some sem ir para uma cópia de conflito ou para a lixeira.

#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::unused_self
)]

mod aleatorio;
mod bancada;

use bancada::Bancada;

fn conteudo(bancada: &bancada::Disco, caminho: &str) -> Vec<u8> {
    bancada.ler(caminho).cloned().unwrap_or_default()
}

#[test]
fn compartilhar_leva_a_arvore_inteira_para_o_outro_lado() {
    let mut b = Bancada::nova();
    b.grava_na_origem("relatório/março.xlsx", b"planilha");
    b.grava_na_origem("relatório/anexos/foto.jpg", b"jpeg");
    b.grava_na_origem("notas.txt", b"oi");
    b.disco_o.criar_pasta("vazia");
    b.rodar();
    assert!(
        b.em_dia(),
        "{:?}\n{:?}",
        b.disco_o.arvore(),
        b.disco_r.arvore()
    );
    assert_eq!(b.baixados, 3);
}

#[test]
fn uma_edicao_em_cada_lado_chega_ao_outro() {
    let mut b = Bancada::nova();
    b.grava_na_origem("a.txt", b"um");
    b.rodar();
    b.grava_na_replica("a.txt", b"dois, editado na replica");
    b.rodar();
    assert_eq!(conteudo(&b.disco_o, "a.txt"), b"dois, editado na replica");
    b.grava_na_origem("a.txt", b"tres, de volta na origem");
    b.rodar();
    assert_eq!(conteudo(&b.disco_r, "a.txt"), b"tres, de volta na origem");
    assert!(b.em_dia());
    assert!(b.disco_o.lixeira.is_empty() && b.disco_r.lixeira.is_empty());
}

#[test]
fn o_que_nasce_na_replica_vai_para_a_origem_com_as_pastas() {
    let mut b = Bancada::nova();
    b.rodar();
    b.grava_na_replica("novos/fundo/arquivo.bin", b"criado na replica");
    b.disco_r.criar_pasta("so-pasta");
    b.rodar();
    assert!(
        b.em_dia(),
        "{:?}\n{:?}",
        b.disco_o.arvore(),
        b.disco_r.arvore()
    );
    assert!(b.disco_o.pastas.contains("novos/fundo"));
}

#[test]
fn offline_as_mudancas_esperam_e_vao_quando_o_canal_volta() {
    let mut b = Bancada::nova();
    b.grava_na_origem("a.txt", b"a");
    b.rodar();
    b.ligado = false;
    for n in 0..100 {
        b.grava_na_replica(
            &format!("offline/{n:03}.txt"),
            format!("arquivo {n}").as_bytes(),
        );
    }
    b.rodar();
    assert_eq!(
        b.replica.pendentes(),
        101,
        "100 arquivos e a subpasta esperam"
    );
    assert!(!b.em_dia());
    b.ligado = true;
    b.rodar();
    assert!(b.em_dia());
    assert_eq!(b.replica.pendentes(), 0);
}

/// O cenário que motivou a regra: o mesmo `.docx` editado nos dois computadores enquanto estavam
/// separados.
#[test]
fn editado_nos_dois_offline_as_duas_versoes_ficam() {
    let mut b = Bancada::nova();
    b.grava_na_origem("proposta.docx", b"v1");
    b.rodar();
    b.ligado = false;
    b.grava_na_origem("proposta.docx", b"v2 da origem");
    b.grava_na_replica("proposta.docx", b"v2 da replica, mais recente");
    b.rodar();
    b.ligado = true;
    b.rodar();
    assert!(
        b.em_dia(),
        "{:?}\n{:?}",
        b.disco_o.arvore(),
        b.disco_r.arvore()
    );
    let (_, arquivos) = b.disco_o.arvore();
    assert_eq!(
        arquivos.len(),
        2,
        "o original e a cópia de conflito: {arquivos:?}"
    );
    assert_eq!(arquivos["proposta.docx"], b"v2 da replica, mais recente");
    let copia = arquivos
        .keys()
        .find(|c| c.starts_with("proposta (conflito DESKTOP "))
        .expect("a versão da origem foi para o lado, com o nome dela");
    assert_eq!(arquivos[copia], b"v2 da origem");
}

#[test]
fn no_conflito_a_versao_mais_recente_da_origem_fica_com_o_nome() {
    let mut b = Bancada::nova();
    b.grava_na_origem("p.txt", b"v1");
    b.rodar();
    b.ligado = false;
    b.grava_na_replica("p.txt", b"replica primeiro");
    b.grava_na_origem("p.txt", b"origem depois");
    b.rodar();
    b.ligado = true;
    b.rodar();
    assert!(b.em_dia());
    let (_, arquivos) = b.disco_r.arvore();
    assert_eq!(arquivos["p.txt"], b"origem depois");
    let copia = arquivos
        .keys()
        .find(|c| c.starts_with("p (conflito NOTEBOOK "))
        .expect("a versão da réplica foi para o lado");
    assert_eq!(arquivos[copia], b"replica primeiro");
    // A réplica não baixou de volta o próprio conteúdo: ele foi movido para o nome da cópia.
    assert_eq!(
        b.baixados, 2,
        "v1 no começo e a versão da origem depois do conflito"
    );
}

#[test]
fn apagar_na_replica_o_que_a_origem_editou_traz_o_arquivo_de_volta() {
    let mut b = Bancada::nova();
    b.grava_na_origem("x.txt", b"v1");
    b.rodar();
    b.ligado = false;
    b.disco_r.remover("x.txt");
    b.grava_na_origem("x.txt", b"v2 editado na origem");
    b.rodar();
    b.ligado = true;
    b.rodar();
    assert!(b.em_dia());
    assert_eq!(conteudo(&b.disco_r, "x.txt"), b"v2 editado na origem");
}

#[test]
fn apagar_na_origem_o_que_a_replica_editou_mantem_a_edicao() {
    let mut b = Bancada::nova();
    b.grava_na_origem("y.txt", b"v1");
    b.rodar();
    b.ligado = false;
    b.disco_o.remover("y.txt");
    b.grava_na_replica("y.txt", b"v2 editado na replica");
    b.rodar();
    b.ligado = true;
    b.rodar();
    assert!(b.em_dia());
    assert_eq!(conteudo(&b.disco_o, "y.txt"), b"v2 editado na replica");
}

#[test]
fn apagar_sem_conflito_leva_para_a_lixeira_e_nao_some() {
    let mut b = Bancada::nova();
    b.grava_na_origem("pasta/z.txt", b"conteudo");
    b.rodar();
    b.disco_r.remover("pasta");
    b.rodar();
    assert!(b.em_dia());
    assert!(b.disco_o.arquivos.is_empty());
    assert!(
        b.disco_o
            .lixeira
            .iter()
            .any(|(c, d)| c == "pasta/z.txt" && d == b"conteudo"),
        "{:?}",
        b.disco_o.lixeira
    );
}

#[test]
fn renomear_na_origem_move_na_replica_sem_baixar_de_novo() {
    let mut b = Bancada::nova();
    b.grava_na_origem("velho.iso", b"um arquivo grande");
    b.rodar();
    assert_eq!(b.baixados, 1);
    let dados = b.disco_o.remover("velho.iso");
    let quando = b.tempo;
    b.disco_o.escrever("novo.iso", &dados[0].1, quando);
    b.rodar();
    assert!(b.em_dia());
    assert_eq!(b.baixados, 1, "o conteúdo já estava na réplica");
}

#[test]
fn o_canal_que_cai_no_meio_de_uma_operacao_nao_perde_nada() {
    let mut b = Bancada::nova();
    b.rodar();
    b.grava_na_replica("a.txt", b"a");
    b.grava_na_replica("b.txt", b"b");
    b.replica.aplicar_retrato(&b.disco_r.retrato());
    // A primeira saiu e o canal caiu antes da resposta.
    let _ = b.replica.proxima().expect("há o que mandar");
    b.replica.canal_caiu();
    b.rodar();
    assert!(b.em_dia());
    assert_eq!(b.replica.pendentes(), 0);
}

#[test]
fn os_temporarios_do_office_nao_viajam() {
    let mut b = Bancada::nova();
    b.rodar();
    b.grava_na_replica("~$proposta.docx", b"trava do word");
    b.grava_na_replica("proposta.docx", b"documento");
    b.rodar();
    assert!(b.disco_o.ler("~$proposta.docx").is_none());
    assert_eq!(conteudo(&b.disco_o, "proposta.docx"), b"documento");
}

#[test]
fn o_indice_sobrevive_a_ser_guardado_e_lido_de_volta() {
    let mut b = Bancada::nova();
    b.grava_na_origem("a/b.txt", b"x");
    b.rodar();
    b.ligado = false;
    b.grava_na_replica("c.txt", b"pendente");
    b.rodar();
    let origem: ir_pasta::Origem =
        postcard::from_bytes(&postcard::to_allocvec(&b.origem).unwrap()).unwrap();
    let replica: ir_pasta::Replica =
        postcard::from_bytes(&postcard::to_allocvec(&b.replica).unwrap()).unwrap();
    assert_eq!(origem, b.origem);
    assert_eq!(replica, b.replica);
    assert_eq!(replica.pendentes(), 1, "a fila offline vai junto");
}
