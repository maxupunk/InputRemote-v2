use super::*;

/// Um DIB `BITMAPINFOHEADER` de 32 bits, BGRA, com as linhas na ordem dada.
fn dib32(largura: i32, altura: i32, pixels_bgra: &[[u8; 4]]) -> Vec<u8> {
    let mut dib = Vec::new();
    dib.extend_from_slice(&40u32.to_le_bytes());
    dib.extend_from_slice(&largura.to_le_bytes());
    dib.extend_from_slice(&altura.to_le_bytes());
    dib.extend_from_slice(&1u16.to_le_bytes());
    dib.extend_from_slice(&32u16.to_le_bytes());
    dib.extend_from_slice(&BI_RGB.to_le_bytes());
    dib.extend_from_slice(&[0; 20]);
    for pixel in pixels_bgra {
        dib.extend_from_slice(pixel);
    }
    dib
}

fn png_rgba(largura: u32, altura: u32, rgba: &[u8]) -> Vec<u8> {
    codificar(&Pixels {
        largura,
        altura,
        rgba: rgba.to_vec(),
    })
    .unwrap()
}

#[test]
fn um_png_vai_ao_dib_e_volta_igual() {
    // Vermelho, verde / azul, meio transparente — de cima para baixo.
    let rgba = [
        255, 0, 0, 255, 0, 255, 0, 255, //
        0, 0, 255, 255, 10, 20, 30, 128,
    ];
    let png = png_rgba(2, 2, &rgba);
    let dib = dib_de_png(&png).unwrap();
    assert_eq!(decodificar_dib(&dib).unwrap().rgba, rgba);
    // E o PNG que volta decodifica nos mesmos pixels: é o que a guarda de eco vê do outro lado.
    assert_eq!(
        decodificar_png(&png_de_dib(&dib).unwrap()).unwrap().rgba,
        rgba
    );
}

#[test]
fn o_dib_sai_de_baixo_para_cima_em_bgra() {
    let png = png_rgba(1, 2, &[1, 2, 3, 4, 5, 6, 7, 8]);
    let dib = dib_de_png(&png).unwrap();
    assert_eq!(
        i32::from_le_bytes(dib[8..12].try_into().unwrap()),
        2,
        "altura positiva"
    );
    // A primeira linha gravada é a de baixo, em BGRA.
    assert_eq!(&dib[40..44], &[7, 6, 5, 8]);
    assert_eq!(&dib[44..48], &[3, 2, 1, 4]);
}

#[test]
fn um_dib_de_cima_para_baixo_e_lido_na_ordem_certa() {
    // Altura negativa: a primeira linha nos bytes é a de cima.
    let dib = dib32(1, -2, &[[0, 0, 255, 255], [255, 0, 0, 255]]);
    let imagem = decodificar_dib(&dib).unwrap();
    assert_eq!(imagem.rgba, vec![255, 0, 0, 255, 0, 0, 255, 255]);
}

#[test]
fn alfa_todo_zerado_e_imagem_opaca() {
    // O byte reservado de quase todo programa: levar o zero a sério daria uma colagem invisível.
    let dib = dib32(2, 1, &[[1, 2, 3, 0], [4, 5, 6, 0]]);
    let imagem = decodificar_dib(&dib).unwrap();
    assert_eq!(imagem.rgba, vec![3, 2, 1, 255, 6, 5, 4, 255]);
}

#[test]
fn alfa_de_verdade_e_respeitado() {
    let dib = dib32(2, 1, &[[1, 2, 3, 0], [4, 5, 6, 200]]);
    let imagem = decodificar_dib(&dib).unwrap();
    assert_eq!(imagem.rgba, vec![3, 2, 1, 0, 6, 5, 4, 200]);
}

#[test]
fn vinte_e_quatro_bits_com_linha_alinhada_a_quatro() {
    // Uma linha de 1 pixel de 24 bits ocupa 3 bytes e tem 1 de enchimento.
    let mut dib = dib32(1, 2, &[]);
    dib[14] = 24;
    dib.extend_from_slice(&[10, 20, 30, 0]); // linha de baixo
    dib.extend_from_slice(&[40, 50, 60, 0]); // linha de cima
    let imagem = decodificar_dib(&dib).unwrap();
    assert_eq!(imagem.rgba, vec![60, 50, 40, 255, 30, 20, 10, 255]);
}

#[test]
fn mascaras_depois_do_cabecalho_curto_sao_lidas() {
    // `BI_BITFIELDS` com `BITMAPINFOHEADER`: as máscaras vêm depois dos 40 bytes, em RGB.
    let mut dib = dib32(1, 1, &[]);
    dib[16] = 3;
    for mascara in [0x0000_00ffu32, 0x0000_ff00, 0x00ff_0000] {
        dib.extend_from_slice(&mascara.to_le_bytes());
    }
    dib.extend_from_slice(&[11, 22, 33, 0]);
    let imagem = decodificar_dib(&dib).unwrap();
    assert_eq!(imagem.rgba, vec![11, 22, 33, 255]);
}

#[test]
fn um_canal_curto_e_esticado_ate_o_branco() {
    assert_eq!(canal(0b1_1111, 0b1_1111), 255);
    assert_eq!(canal(0, 0b1_1111), 0);
    assert_eq!(canal(0xff00, 0xff00), 255);
}

#[test]
fn paleta_e_cabecalho_truncado_nao_atravessam() {
    let mut dib = dib32(1, 1, &[[0, 0, 0, 0]]);
    dib[14] = 8;
    assert!(matches!(
        png_de_dib(&dib),
        Err(ClipError::FormatoNaoSuportado)
    ));
    assert!(matches!(
        png_de_dib(&[40, 0, 0]),
        Err(ClipError::FormatoNaoSuportado)
    ));
    // Pixels a menos que o cabeçalho promete.
    let curto = dib32(4, 4, &[[0; 4]]);
    assert!(matches!(
        png_de_dib(&curto),
        Err(ClipError::FormatoNaoSuportado)
    ));
}

#[test]
fn um_cabecalho_gigante_nao_aloca_pelo_que_diz() {
    let dib = dib32(100_000, 100_000, &[]);
    assert!(matches!(
        png_de_dib(&dib),
        Err(ClipError::FormatoNaoSuportado)
    ));
}

#[test]
fn bytes_que_nao_sao_png_sao_recusados() {
    assert!(matches!(
        dib_de_png(b"nada disto"),
        Err(ClipError::FormatoNaoSuportado)
    ));
}

#[test]
fn png_em_cinza_vira_rgba() {
    let mut saida = Vec::new();
    let mut codificador = png::Encoder::new(&mut saida, 2, 1);
    codificador.set_color(png::ColorType::Grayscale);
    codificador.set_depth(png::BitDepth::Eight);
    let mut escritor = codificador.write_header().unwrap();
    escritor.write_image_data(&[0, 200]).unwrap();
    escritor.finish().unwrap();
    let imagem = decodificar_png(&saida).unwrap();
    assert_eq!(imagem.rgba, vec![0, 0, 0, 255, 200, 200, 200, 255]);
}
