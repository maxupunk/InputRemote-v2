//! Os quadros do ícone da bandeja, prontos para o Windows.
//!
//! Desenhados por `recursos/gerar-icones.py`, uma tira por tamanho, com os quadros lado a lado na
//! ordem de [`QUADROS`]. A tira certa é a do tamanho que a bandeja usa na escala da tela: 16 px a
//! 100%, 20 a 125%, 24 a 150%, 32 a 200%. Dar ao Windows um ícone maior para ele reduzir borra o
//! selo, que é justamente o detalhe que importa.
//!
//! O tamanho vem do Windows (`SM_CXSMICON`), e não da escala da janela: a interface que sobe com o
//! login começa escondida, e janela escondida não tem escala.

#![allow(unsafe_code)]

use super::aparencia::Aparencia;

/// Os quadros de cada tira, nesta ordem — contrato com `recursos/gerar-icones.py`.
const QUADROS: usize = 12;
/// Onde o arco girando começa, e quantos quadros ele tem.
const TRABALHANDO: usize = 4;
const VOLTA: usize = 8;

/// As tiras, por tamanho.
const TIRAS: [(u32, &[u8]); 4] = [
    (16, include_bytes!("../../../../recursos/bandeja-16.png")),
    (20, include_bytes!("../../../../recursos/bandeja-20.png")),
    (24, include_bytes!("../../../../recursos/bandeja-24.png")),
    (32, include_bytes!("../../../../recursos/bandeja-32.png")),
];

/// Os quadros do ícone, num tamanho.
pub struct Icones {
    quadros: Vec<tray_icon::Icon>,
}

impl std::fmt::Debug for Icones {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Icones")
            .field("quadros", &self.quadros.len())
            .finish()
    }
}

impl Icones {
    /// Os quadros do tamanho que a bandeja usa. `None` se a tira não abrir — aí o ícone fica o de
    /// sempre, sem estados.
    #[must_use]
    pub fn da_bandeja() -> Option<Self> {
        Self::de_lado(lado_da_bandeja())
    }

    /// A menor tira que cobre `lado`; acima da maior, a maior.
    fn de_lado(lado: u32) -> Option<Self> {
        let (lado, tira) = TIRAS
            .iter()
            .find(|(tamanho, _)| *tamanho >= lado)
            .or_else(|| TIRAS.last())?;
        let quadros = quadros(tira, *lado)?;
        Some(Self { quadros })
    }

    /// O quadro desta aparência. `batida` conta as batidas da bandeja, e faz o arco girar.
    #[must_use]
    pub fn quadro(&self, aparencia: Aparencia, batida: usize) -> Option<tray_icon::Icon> {
        let indice = match aparencia {
            Aparencia::Normal => 0,
            Aparencia::Inativo => 1,
            Aparencia::Feito => 2,
            Aparencia::Problema => 3,
            Aparencia::Trabalhando => TRABALHANDO + batida % VOLTA,
        };
        self.quadros.get(indice).cloned()
    }
}

/// O lado, em pixels, dos ícones pequenos — os da bandeja — na escala desta tela.
fn lado_da_bandeja() -> u32 {
    use windows_sys::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_CXSMICON};
    // SAFETY: a chamada só lê uma métrica do sistema e não tem pré-condição.
    let lado = unsafe { GetSystemMetrics(SM_CXSMICON) };
    u32::try_from(lado)
        .ok()
        .filter(|lado| *lado > 0)
        .unwrap_or(16)
}

/// Lê a tira e a corta em quadros.
fn quadros(tira: &[u8], lado: u32) -> Option<Vec<tray_icon::Icon>> {
    let mut leitor = png::Decoder::new(std::io::Cursor::new(tira))
        .read_info()
        .ok()?;
    let mut pixels = vec![0; leitor.output_buffer_size()?];
    let info = leitor.next_frame(&mut pixels).ok()?;
    if info.color_type != png::ColorType::Rgba || info.bit_depth != png::BitDepth::Eight {
        return None;
    }
    let lado = usize::try_from(lado).ok()?;
    let linha = info.line_size;
    (0..QUADROS)
        .map(|quadro| {
            let mut rgba = Vec::with_capacity(lado * lado * 4);
            for y in 0..lado {
                let inicio = y * linha + quadro * lado * 4;
                rgba.extend_from_slice(pixels.get(inicio..inicio + lado * 4)?);
            }
            let lado = u32::try_from(lado).ok()?;
            tray_icon::Icon::from_rgba(rgba, lado, lado).ok()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toda_tira_abre_com_os_doze_quadros() {
        for (lado, tira) in TIRAS {
            let quadros = quadros(tira, lado).expect("a tira abre");
            assert_eq!(quadros.len(), QUADROS, "tira de {lado}");
        }
    }

    #[test]
    fn o_lado_escolhe_a_tira_e_a_maior_cobre_o_resto() {
        assert!(Icones::de_lado(16).is_some());
        assert!(
            Icones::de_lado(28).is_some(),
            "175%: a de 32, reduzida pelo Windows"
        );
        assert!(Icones::de_lado(48).is_some(), "acima de 200%, a de 32");
        assert!(Icones::da_bandeja().is_some());
    }
}
