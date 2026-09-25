//! O arranjo de telas que o injetor usa para pôr o ponteiro no monitor certo.
//!
//! A posição que chega para injetar é uma fração **dentro de um monitor**; o `SendInput` com
//! `MOUSEEVENTF_VIRTUALDESK` e o eixo absoluto do `uinput` querem a fração do desktop virtual
//! inteiro. Os dois injetores entregavam uma como se fosse a outra, o que só acerta com uma tela. A
//! conversão é de `ir-geometry`, a mesma que a sessão usa; aqui fica só o arranjo guardado.

use ir_geometry::Desktop;
use ir_proto::input::PointerPosition;
use ir_proto::screens::ScreenLayout;

/// O arranjo local, se já foi dado.
#[derive(Debug, Clone, Default)]
pub(crate) struct ArranjoLocal(Option<Desktop>);

impl ArranjoLocal {
    /// Sem arranjo ainda.
    pub(crate) const fn nenhum() -> Self {
        Self(None)
    }

    /// Passa a usar este arranjo. Um arranjo sem monitor utilizável volta ao "não sei".
    pub(crate) fn usar(&mut self, telas: &ScreenLayout) {
        self.0 = Desktop::from_layout(telas);
    }

    /// A posição, no referencial do injetor: `0..=65535` sobre o desktop virtual inteiro.
    ///
    /// Sem arranjo, a posição vale como se houvesse um monitor só — o que era o comportamento de
    /// antes, e o certo numa máquina com uma tela.
    #[doc = "hot path"]
    pub(crate) fn no_desktop_virtual(&self, posicao: PointerPosition) -> (u16, u16) {
        self.0.as_ref().map_or((posicao.x, posicao.y), |desktop| {
            desktop.to_virtual_fraction(posicao)
        })
    }
}

#[cfg(test)]
mod tests {
    use ir_proto::ids::MonitorId;
    use ir_proto::screens::MonitorInfo;

    use super::*;

    fn monitor(id: u8, x: i32, largura: u32) -> MonitorInfo {
        MonitorInfo {
            id: MonitorId(id),
            x,
            y: 0,
            width: largura,
            height: 1080,
            scale_permille: 1000,
            primary: id == 0,
        }
    }

    fn em(monitor: u8, x: u16) -> PointerPosition {
        PointerPosition {
            monitor: MonitorId(monitor),
            x,
            y: 0,
        }
    }

    #[test]
    fn sem_arranjo_a_posicao_passa_como_esta() {
        let arranjo = ArranjoLocal::default();
        assert_eq!(arranjo.no_desktop_virtual(em(3, 1234)), (1234, 0));
    }

    #[test]
    fn com_uma_tela_a_posicao_passa_como_esta() {
        let mut arranjo = ArranjoLocal::default();
        arranjo.usar(&ScreenLayout::single(1920, 1080).expect("arranjo"));
        for x in [0, 1, 32_767, u16::MAX] {
            let (vx, _) = arranjo.no_desktop_virtual(em(0, x));
            assert!(vx.abs_diff(x) <= 1, "{x} -> {vx}");
        }
    }

    #[test]
    fn com_dois_monitores_o_segundo_fica_a_direita_do_primeiro() {
        // Regressão: a fração do segundo monitor era entregue como fração do desktop inteiro, e o
        // ponteiro caía no primeiro.
        let mut arranjo = ArranjoLocal::default();
        let telas =
            ScreenLayout::new(vec![monitor(0, 0, 1920), monitor(1, 1920, 1920)]).expect("arranjo");
        arranjo.usar(&telas);
        let (inicio_do_segundo, _) = arranjo.no_desktop_virtual(em(1, 0));
        assert!(
            inicio_do_segundo.abs_diff(u16::MAX / 2) <= 20,
            "{inicio_do_segundo}"
        );
        assert_eq!(arranjo.no_desktop_virtual(em(1, u16::MAX)).0, u16::MAX);
        assert_eq!(arranjo.no_desktop_virtual(em(0, 0)).0, 0);
    }
}
