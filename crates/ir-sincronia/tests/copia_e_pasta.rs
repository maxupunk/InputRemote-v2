//! A cópia (Ctrl+C) e a pasta compartilhada juntas: o mesmo conteúdo não atravessa a rede duas
//! vezes. Dois ajudantes de verdade, com a bancada comum, contando o que atravessa.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing
)]

mod comum;

use comum::{Bancada, escrever};
use ir_ipc::pastas::ComandoDePasta;

#[test]
fn copiar_de_dentro_da_pasta_poe_no_clipboard_do_outro_os_arquivos_dele() {
    let mut b = Bancada::nova("copiar-de-dentro");
    let (_, raiz_b) = b.compartilhar(&[("fotos/img2.jpg", b"a foto"), ("notas.txt", b"oi")]);
    let pasta = b.a.pastas.resumo()[0].id;
    b.trechos_de_a = 0;
    let copiado = ComandoDePasta::Copiado {
        pasta,
        caminhos: vec![
            "fotos/img2.jpg".to_owned(),
            "nao-existe-la.txt".to_owned(),
            "../fora-da-pasta".to_owned(),
        ],
    };
    b.a.pastas.comando(copiado, &mut b.a.fila).unwrap();
    b.rodar();
    let no_clipboard =
        b.b.pastas
            .tirar_do_clipboard()
            .expect("o clipboard de B recebeu");
    let esperado = raiz_b.join("fotos").join("img2.jpg");
    assert_eq!(no_clipboard, vec![esperado.to_string_lossy().into_owned()]);
    assert_eq!(b.trechos_de_a, 0, "nenhum byte atravessou pela cópia");
    assert!(b.b.pastas.tirar_do_clipboard().is_none(), "uma vez só");
}

#[test]
fn o_que_b_cola_na_pasta_e_a_copiou_de_fora_dela_nao_atravessa_de_novo() {
    let mut b = Bancada::nova("colar-na-pasta");
    let (raiz_a, raiz_b) = b.compartilhar(&[("a.txt", b"1")]);
    // A copiou img2.jpg de fora da pasta; a cópia já a levou a B.
    let foto: Vec<u8> = (0..300_000u32).map(|i| (i % 253) as u8).collect();
    let original = b.base.join("desktop").join("Imagens").join("img2.jpg");
    escrever(&original, &foto);
    ir_sincronia::conhecidos::registrar(&b.base.join("desktop").join("estado"), &[original]);
    // B cola na pasta o que chegou.
    b.blocos_de_b = 0;
    escrever(&raiz_b.join("img2.jpg"), &foto);
    b.rodar();
    assert_eq!(std::fs::read(raiz_a.join("img2.jpg")).unwrap(), foto);
    assert_eq!(
        b.blocos_de_b, 0,
        "a origem achou o conteúdo e não pediu os bytes"
    );
}

#[test]
fn o_que_a_cola_na_pasta_e_b_ja_recebeu_pela_copia_nao_atravessa_de_novo() {
    let mut b = Bancada::nova("colar-na-origem");
    let (raiz_a, raiz_b) = b.compartilhar(&[("a.txt", b"1")]);
    // A copiou img3.jpg; a cópia a levou à pasta de recebidos de B.
    let foto: Vec<u8> = (0..300_000u32).map(|i| (i % 241) as u8).collect();
    let recebido = b.base.join("notebook").join("recebidos").join("img3.jpg");
    escrever(&recebido, &foto);
    ir_sincronia::conhecidos::registrar(&b.base.join("notebook").join("estado"), &[recebido]);
    // A também cola na pasta.
    b.trechos_de_a = 0;
    escrever(&raiz_a.join("img3.jpg"), &foto);
    b.rodar();
    assert_eq!(std::fs::read(raiz_b.join("img3.jpg")).unwrap(), foto);
    assert_eq!(
        b.trechos_de_a, 0,
        "a réplica achou o conteúdo e não o baixou"
    );
}
