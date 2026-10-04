//! Os quadros do ícone, decodificados, para os dois sistemas.
//!
//! Desenhados por `recursos/gerar-icones.py`, uma tira por tamanho, com os quadros lado a lado na
//! ordem abaixo — contrato com o gerador. O Windows usa a tira do tamanho da bandeja dele; o Linux
//! entrega todas, e o painel escolhe.

use super::aparencia::Aparencia;

/// Os quadros de cada tira: normal, inativo, feito, problema e os oito do arco girando.
pub const QUADROS: usize = 12;
/// Onde o arco girando começa, e quantos quadros ele tem.
const TRABALHANDO: usize = 4;
const VOLTA: usize = 8;

/// As tiras, por tamanho: a bandeja a 100%, 125%, 150% e 200% de escala.
const TIRAS: [(u32, &[u8]); 4] = [
    (16, include_bytes!("../../../../recursos/bandeja-16.png")),
    (20, include_bytes!("../../../../recursos/bandeja-20.png")),
    (24, include_bytes!("../../../../recursos/bandeja-24.png")),
    (32, include_bytes!("../../../../recursos/bandeja-32.png")),
];

/// Um quadro: o lado, em pixels, e os pixels em RGBA.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Quadro {
    /// O lado do quadrado.
    pub lado: u32,
    /// Os pixels, linha a linha, quatro bytes cada: vermelho, verde, azul, opacidade.
    pub rgba: Vec<u8>,
}

/// O índice do quadro desta aparência. `batida` faz o arco girar.
#[must_use]
pub const fn indice(aparencia: Aparencia, batida: usize) -> usize {
    match aparencia {
        Aparencia::Normal => 0,
        Aparencia::Inativo => 1,
        Aparencia::Feito => 2,
        Aparencia::Problema => 3,
        Aparencia::Trabalhando => TRABALHANDO + batida % VOLTA,
    }
}

/// Os quadros da menor tira que cobre `lado`; acima da maior, os da maior. Dar ao sistema um
/// ícone maior para ele reduzir borra o selo, que é justamente o detalhe que importa.
#[must_use]
pub fn que_cobrem(lado: u32) -> Option<Vec<Quadro>> {
    let (lado, tira) = TIRAS
        .iter()
        .find(|(tamanho, _)| *tamanho >= lado)
        .or_else(|| TIRAS.last())?;
    decodificar(tira, *lado)
}

/// Os quadros de todas as tiras, do menor tamanho ao maior.
#[must_use]
pub fn de_todos_os_tamanhos() -> Vec<Vec<Quadro>> {
    TIRAS
        .iter()
        .filter_map(|(lado, tira)| decodificar(tira, *lado))
        .collect()
}

/// Lê a tira e a corta em quadros.
fn decodificar(tira: &[u8], lado: u32) -> Option<Vec<Quadro>> {
    let mut leitor = png::Decoder::new(std::io::Cursor::new(tira))
        .read_info()
        .ok()?;
    let mut pixels = vec![0; leitor.output_buffer_size()?];
    let info = leitor.next_frame(&mut pixels).ok()?;
    if info.color_type != png::ColorType::Rgba || info.bit_depth != png::BitDepth::Eight {
        return None;
    }
    let largura = usize::try_from(lado).ok()? * 4;
    let linha = info.line_size;
    (0..QUADROS)
        .map(|quadro| {
            let mut rgba = Vec::with_capacity(largura * largura / 4);
            for y in 0..largura / 4 {
                let inicio = y * linha + quadro * largura;
                rgba.extend_from_slice(pixels.get(inicio..inicio + largura)?);
            }
            Some(Quadro { lado, rgba })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toda_tira_abre_com_os_doze_quadros_do_tamanho_certo() {
        let todas = de_todos_os_tamanhos();
        assert_eq!(todas.len(), TIRAS.len());
        for (quadros, (lado, _)) in todas.iter().zip(TIRAS) {
            assert_eq!(quadros.len(), QUADROS, "tira de {lado}");
            for quadro in quadros {
                assert_eq!(quadro.lado, lado);
                assert_eq!(quadro.rgba.len(), (lado * lado * 4) as usize);
            }
        }
    }

    #[test]
    fn o_lado_escolhe_a_tira_e_a_maior_cobre_o_resto() {
        assert_eq!(que_cobrem(16).unwrap()[0].lado, 16);
        assert_eq!(
            que_cobrem(28).unwrap()[0].lado,
            32,
            "175%: a de 32, reduzida"
        );
        assert_eq!(
            que_cobrem(48).unwrap()[0].lado,
            32,
            "acima de 200%, a de 32"
        );
    }

    #[test]
    fn o_arco_gira_e_o_resto_fica_parado() {
        assert_eq!(indice(Aparencia::Feito, 7), indice(Aparencia::Feito, 0));
        let volta: Vec<usize> = (0..9).map(|b| indice(Aparencia::Trabalhando, b)).collect();
        assert_eq!(volta.first(), volta.last(), "oito quadros por volta");
        assert!(volta.iter().all(|q| (TRABALHANDO..QUADROS).contains(q)));
    }
}
