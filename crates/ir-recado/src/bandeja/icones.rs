//! Os quadros do ícone da bandeja, prontos para o Windows.
//!
//! A tira é a do tamanho que o próprio Windows diz para os ícones pequenos (`SM_CXSMICON`), e não
//! a da escala da janela: a interface que sobe com o login começa escondida, e janela escondida não
//! tem escala.

#![allow(unsafe_code)]

use super::quadros;

/// Os quadros do ícone, no tamanho da bandeja.
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
        let quadros = quadros::que_cobrem(lado_da_bandeja())?
            .into_iter()
            .map(|q| tray_icon::Icon::from_rgba(q.rgba, q.lado, q.lado).ok())
            .collect::<Option<Vec<_>>>()?;
        Some(Self { quadros })
    }

    /// O quadro deste índice ([`quadros::indice`]).
    #[must_use]
    pub fn quadro(&self, indice: usize) -> Option<tray_icon::Icon> {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn os_quadros_da_bandeja_desta_tela_abrem() {
        let icones = Icones::da_bandeja().expect("abre");
        assert!(icones.quadro(quadros::QUADROS - 1).is_some());
        assert!(icones.quadro(quadros::QUADROS).is_none());
    }
}
