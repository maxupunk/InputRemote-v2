//! A imagem do clipboard: PNG no protocolo, DIB no Windows.
//!
//! O protocolo tem uma forma só para imagem, PNG ([`Conteudo::Imagem`](crate::Conteudo)), pelo
//! mesmo motivo do texto em LF: a guarda de eco compara resumos, e a forma canônica é o que faz a
//! cópia publicada voltar reconhecível. O Linux já fala PNG (`image/png` no Wayland); o Windows fala
//! DIB (`CF_DIB`), e é a conversão entre os dois que mora aqui.
//!
//! Sem E/S e sem `unsafe`: são bytes para bytes, e por isso testáveis em qualquer plataforma. O
//! backend do Windows só entrega e recebe o bloco do clipboard.
//!
//! # O que se aceita de DIB
//!
//! 24 e 32 bits por pixel, sem compressão ou com máscaras de cor (`BI_BITFIELDS`), de baixo para
//! cima ou de cima para baixo. É o que capturas de tela, navegadores e editores de imagem põem. Paleta
//! (8 bits ou menos) e JPEG embutido ficam de fora: [`ClipError::FormatoNaoSuportado`], e a cópia
//! simplesmente não atravessa — não uma imagem errada do outro lado.

use std::io::Cursor;

use crate::error::{ClipError, Result};

/// O maior número de pixels que se converte.
///
/// Uma imagem de 40 megapixels ocupa 160 MB em RGBA. Acima disso é mais provável um cabeçalho
/// corrompido que uma captura de tela, e alocar pelo que ele diz derrubaria o ajudante.
pub const TETO_DE_PIXELS: u64 = 40_000_000;

/// `BI_RGB`: pixels sem compressão.
const BI_RGB: u32 = 0;
/// `BI_BITFIELDS`: pixels com máscaras de cor declaradas.
const BI_BITFIELDS: u32 = 3;
/// O tamanho de `BITMAPINFOHEADER`, o cabeçalho que escrevemos.
const CABECALHO: usize = 40;
/// O deslocamento das máscaras num cabeçalho V4/V5, que as traz dentro dele.
const MASCARAS_NO_CABECALHO: usize = 40;

/// Uma imagem decodificada: RGBA de 8 bits, linha a linha de cima para baixo.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Pixels {
    largura: u32,
    altura: u32,
    rgba: Vec<u8>,
}

/// As máscaras de cor de um DIB de 32 bits.
#[derive(Debug, Clone, Copy)]
struct Mascaras {
    vermelho: u32,
    verde: u32,
    azul: u32,
    alfa: u32,
}

/// As máscaras de quem não declara nenhuma: BGRA, com o alfa no byte de cima.
const BGRA: Mascaras = Mascaras {
    vermelho: 0x00ff_0000,
    verde: 0x0000_ff00,
    azul: 0x0000_00ff,
    alfa: 0xff00_0000,
};

/// Um DIB (`CF_DIB` ou `CF_DIBV5`) como PNG.
///
/// # Errors
///
/// [`ClipError::FormatoNaoSuportado`] para o que este módulo não converte — paleta, compressão,
/// cabeçalho truncado ou grande demais.
pub fn png_de_dib(dib: &[u8]) -> Result<Vec<u8>> {
    codificar(&decodificar_dib(dib)?)
}

/// Um PNG como DIB de 32 bits (`CF_DIB`), de baixo para cima, como o Windows o espera.
///
/// # Errors
///
/// [`ClipError::FormatoNaoSuportado`] se os bytes não forem um PNG que se decodifique.
pub fn dib_de_png(png: &[u8]) -> Result<Vec<u8>> {
    let imagem = decodificar_png(png)?;
    let tamanho = imagem.rgba.len();
    let mut dib = Vec::with_capacity(CABECALHO + tamanho);
    let altura = i32::try_from(imagem.altura).map_err(|_| nao_suportado())?;
    let largura = i32::try_from(imagem.largura).map_err(|_| nao_suportado())?;
    dib.extend_from_slice(&u32::try_from(CABECALHO).unwrap_or(40).to_le_bytes());
    dib.extend_from_slice(&largura.to_le_bytes());
    // Altura positiva: de baixo para cima, a forma que todo leitor de `CF_DIB` aceita.
    dib.extend_from_slice(&altura.to_le_bytes());
    dib.extend_from_slice(&1u16.to_le_bytes()); // planos
    dib.extend_from_slice(&32u16.to_le_bytes()); // bits por pixel
    dib.extend_from_slice(&BI_RGB.to_le_bytes());
    dib.extend_from_slice(&u32::try_from(tamanho).unwrap_or(0).to_le_bytes());
    dib.extend_from_slice(&[0; 16]); // resolução e paleta: nenhuma
    let linha = imagem.largura as usize * 4;
    for linha_rgba in imagem.rgba.chunks_exact(linha.max(1)).rev() {
        for pixel in linha_rgba.chunks_exact(4) {
            if let [r, g, b, a] = *pixel {
                dib.extend_from_slice(&[b, g, r, a]);
            }
        }
    }
    Ok(dib)
}

/// Lê o cabeçalho e os pixels de um DIB.
fn decodificar_dib(dib: &[u8]) -> Result<Pixels> {
    let tamanho_do_cabecalho = ler_u32(dib, 0)? as usize;
    if tamanho_do_cabecalho < CABECALHO {
        return Err(nao_suportado());
    }
    let largura = ler_i32(dib, 4)?;
    let altura_declarada = ler_i32(dib, 8)?;
    let bits = ler_u16(dib, 14)?;
    let compressao = ler_u32(dib, 16)?;
    let cores_usadas = ler_u32(dib, 32)? as usize;
    let largura = u32::try_from(largura).map_err(|_| nao_suportado())?;
    let altura = altura_declarada.unsigned_abs();
    if largura == 0 || altura == 0 || u64::from(largura) * u64::from(altura) > TETO_DE_PIXELS {
        return Err(nao_suportado());
    }
    let (mascaras, mut inicio) = mascaras_e_inicio(dib, bits, compressao, tamanho_do_cabecalho)?;
    // Uma paleta declarada num DIB de cor verdadeira é só sugestão, mas ocupa lugar antes dos pixels.
    inicio += cores_usadas.min(256) * 4;
    let bytes_por_pixel = usize::from(bits / 8);
    // Cada linha é alinhada a 4 bytes.
    let passo = (largura as usize * bytes_por_pixel).div_ceil(4) * 4;
    let dados = dib
        .get(inicio..inicio + passo * altura as usize)
        .ok_or_else(nao_suportado)?;
    let mut rgba = Vec::with_capacity(largura as usize * altura as usize * 4);
    let de_baixo_para_cima = altura_declarada > 0;
    for y in 0..altura as usize {
        let origem = if de_baixo_para_cima {
            altura as usize - 1 - y
        } else {
            y
        };
        let linha = dados
            .get(origem * passo..(origem + 1) * passo)
            .ok_or_else(nao_suportado)?;
        for pixel in linha.chunks_exact(bytes_por_pixel).take(largura as usize) {
            rgba.extend_from_slice(&cor(pixel, mascaras));
        }
    }
    sem_alfa_e_opaca(&mut rgba);
    Ok(Pixels {
        largura,
        altura,
        rgba,
    })
}

/// As máscaras de cor e onde os pixels começam, pelo formato declarado.
fn mascaras_e_inicio(
    dib: &[u8],
    bits: u16,
    compressao: u32,
    tamanho_do_cabecalho: usize,
) -> Result<(Mascaras, usize)> {
    Ok(match (bits, compressao) {
        (24 | 32, BI_RGB) => (BGRA, tamanho_do_cabecalho),
        (32, BI_BITFIELDS) if tamanho_do_cabecalho >= MASCARAS_NO_CABECALHO + 16 => (
            ler_mascaras(dib, MASCARAS_NO_CABECALHO)?,
            tamanho_do_cabecalho,
        ),
        // Cabeçalho curto: as três máscaras vêm logo depois dele, sem a do alfa.
        (32, BI_BITFIELDS) => {
            let mascaras = ler_mascaras(dib, tamanho_do_cabecalho)?;
            let sem_alfa = Mascaras {
                alfa: 0,
                ..mascaras
            };
            (sem_alfa, tamanho_do_cabecalho + 12)
        }
        _ => return Err(nao_suportado()),
    })
}

/// Um pixel de 24 ou 32 bits em RGBA.
fn cor(pixel: &[u8], mascaras: Mascaras) -> [u8; 4] {
    match *pixel {
        [b, g, r] => [r, g, b, 0xff],
        [a, b, c, d] => {
            let valor = u32::from_le_bytes([a, b, c, d]);
            [
                canal(valor, mascaras.vermelho),
                canal(valor, mascaras.verde),
                canal(valor, mascaras.azul),
                if mascaras.alfa == 0 {
                    0xff
                } else {
                    canal(valor, mascaras.alfa)
                },
            ]
        }
        _ => [0, 0, 0, 0xff],
    }
}

/// O valor de um canal, levado a 8 bits.
fn canal(valor: u32, mascara: u32) -> u8 {
    if mascara == 0 {
        return 0;
    }
    let bruto = (valor & mascara) >> mascara.trailing_zeros();
    let largura = (mascara >> mascara.trailing_zeros()).count_ones();
    let em_8 = if largura >= 8 {
        bruto >> (largura - 8)
    } else {
        // Estica a faixa curta até 255, para um canal de 5 bits cheio virar branco, e não cinza.
        bruto * 255 / ((1 << largura) - 1)
    };
    u8::try_from(em_8).unwrap_or(u8::MAX)
}

/// Um DIB de 32 bits com o alfa todo zerado é opaco, e não invisível.
///
/// É o que a maioria dos programas põe: o byte "reservado" zerado. Levar o zero a sério daria uma
/// imagem transparente do outro lado — o defeito clássico de colar uma captura e não ver nada.
fn sem_alfa_e_opaca(rgba: &mut [u8]) {
    let tem_alfa = rgba
        .chunks_exact(4)
        .any(|p| p.get(3).is_some_and(|a| *a != 0));
    if !tem_alfa {
        for pixel in rgba.chunks_exact_mut(4) {
            if let Some(a) = pixel.get_mut(3) {
                *a = 0xff;
            }
        }
    }
}

/// Codifica RGBA em PNG.
fn codificar(imagem: &Pixels) -> Result<Vec<u8>> {
    let mut saida = Vec::new();
    let mut codificador = png::Encoder::new(&mut saida, imagem.largura, imagem.altura);
    codificador.set_color(png::ColorType::Rgba);
    codificador.set_depth(png::BitDepth::Eight);
    let mut escritor = codificador.write_header().map_err(|e| sistema(&e))?;
    escritor
        .write_image_data(&imagem.rgba)
        .map_err(|e| sistema(&e))?;
    escritor.finish().map_err(|e| sistema(&e))?;
    Ok(saida)
}

/// Decodifica um PNG qualquer — paleta, cinza, 16 bits — em RGBA de 8 bits.
fn decodificar_png(png: &[u8]) -> Result<Pixels> {
    let mut decodificador = png::Decoder::new_with_limits(
        Cursor::new(png),
        png::Limits {
            bytes: usize::try_from(TETO_DE_PIXELS * 4).unwrap_or(usize::MAX),
        },
    );
    decodificador.set_transformations(png::Transformations::normalize_to_color8());
    let mut leitor = decodificador.read_info().map_err(|_| nao_suportado())?;
    let tamanho = leitor.output_buffer_size().ok_or_else(nao_suportado)?;
    let mut bruto = vec![0; tamanho];
    let info = leitor.next_frame(&mut bruto).map_err(|_| nao_suportado())?;
    if u64::from(info.width) * u64::from(info.height) > TETO_DE_PIXELS {
        return Err(nao_suportado());
    }
    bruto.truncate(info.buffer_size());
    let rgba = match info.color_type {
        png::ColorType::Rgba => bruto,
        png::ColorType::Rgb => bruto
            .chunks_exact(3)
            .flat_map(|p| match *p {
                [r, g, b] => [r, g, b, 0xff],
                _ => [0; 4],
            })
            .collect(),
        png::ColorType::GrayscaleAlpha => bruto
            .chunks_exact(2)
            .flat_map(|p| match *p {
                [v, a] => [v, v, v, a],
                _ => [0; 4],
            })
            .collect(),
        png::ColorType::Grayscale => bruto.iter().flat_map(|&v| [v, v, v, 0xff]).collect(),
        png::ColorType::Indexed => return Err(nao_suportado()),
    };
    Ok(Pixels {
        largura: info.width,
        altura: info.height,
        rgba,
    })
}

fn ler_mascaras(dib: &[u8], em: usize) -> Result<Mascaras> {
    Ok(Mascaras {
        vermelho: ler_u32(dib, em)?,
        verde: ler_u32(dib, em + 4)?,
        azul: ler_u32(dib, em + 8)?,
        alfa: ler_u32(dib, em + 12).unwrap_or(0),
    })
}

fn ler_u32(bytes: &[u8], em: usize) -> Result<u32> {
    match bytes.get(em..em + 4) {
        Some(&[a, b, c, d]) => Ok(u32::from_le_bytes([a, b, c, d])),
        _ => Err(nao_suportado()),
    }
}

fn ler_i32(bytes: &[u8], em: usize) -> Result<i32> {
    ler_u32(bytes, em).map(|v| i32::from_le_bytes(v.to_le_bytes()))
}

fn ler_u16(bytes: &[u8], em: usize) -> Result<u16> {
    match bytes.get(em..em + 2) {
        Some(&[a, b]) => Ok(u16::from_le_bytes([a, b])),
        _ => Err(nao_suportado()),
    }
}

const fn nao_suportado() -> ClipError {
    ClipError::FormatoNaoSuportado
}

fn sistema(erro: &png::EncodingError) -> ClipError {
    ClipError::Sistema(erro.to_string())
}

#[cfg(test)]
mod tests;
