//! O ícone na bandeja: como ele fica pelo que acontece ([`aparencia`]), os quadros que o desenham
//! ([`quadros`]) e a [`Vitrine`], que junta os dois a cada batida.
//!
//! Quem põe o ícone na bandeja é de cada sistema: no Windows, a interface, que mora lá
//! ([`icones`]); no Linux, o ajudante de clipboard, pelo `StatusNotifierItem`
//! (`crate::linux::bandeja`). A decisão e os quadros são os mesmos nos dois.

pub mod aparencia;
#[cfg(windows)]
pub mod icones;
pub mod quadros;

use std::time::Instant;

use aparencia::{Aparencia, Retrato, Selo};

/// O que o ícone mostra, batida a batida.
#[derive(Debug, Default)]
pub struct Vitrine {
    selo: Selo,
    /// As batidas desde o começo, que fazem o arco girar.
    batida: usize,
    /// O último quadro posto, para não repor o mesmo a cada batida.
    ultimo: Option<usize>,
}

/// O resultado de uma batida.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Passo {
    /// Como o ícone está.
    pub aparencia: Aparencia,
    /// O quadro a pôr, quando ele mudou; `None` quando o que está lá já serve.
    pub quadro: Option<usize>,
}

impl Vitrine {
    /// Uma batida, pelo que acontece agora.
    pub fn passo(&mut self, retrato: &Retrato, agora: Instant) -> Passo {
        let aparencia = self.selo.aparencia(retrato, agora);
        self.batida = self.batida.wrapping_add(1);
        let quadro = quadros::indice(aparencia, self.batida);
        let mudou = self.ultimo != Some(quadro);
        self.ultimo = Some(quadro);
        Passo {
            aparencia,
            quadro: mudou.then_some(quadro),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn so_pede_quadro_novo_quando_ele_muda_e_o_arco_muda_a_cada_batida() {
        let mut vitrine = Vitrine::default();
        let parado = Retrato {
            parado: false,
            atravessando: false,
            copia: None,
            janela_visivel: false,
        };
        let agora = Instant::now();
        assert_eq!(vitrine.passo(&parado, agora).quadro, Some(0));
        assert_eq!(vitrine.passo(&parado, agora).quadro, None);
        let andando = Retrato {
            atravessando: true,
            ..parado
        };
        let a = vitrine.passo(&andando, agora).quadro;
        let b = vitrine.passo(&andando, agora).quadro;
        assert!(a.is_some() && b.is_some() && a != b, "{a:?} {b:?}");
    }
}
