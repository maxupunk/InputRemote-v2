//! A entrada local de uma máquina: o que se injeta nela e o que se captura dela.
//!
//! Não viaja entre as máquinas — o fio tem as mensagens do canal de entrada. Viaja entre os
//! processos de **uma** máquina: a sessão pede a injeção, o serviço a repassa ao agente, e o agente
//! a entrega ao injetor; no sentido contrário, a captura sobe até a sessão. Eram três enums de
//! injeção e dois de captura, um por crate, e seis traduções com um curinga que descartaria calado
//! uma variante nova. Agora é um tipo de cada, e nenhuma tradução.
//!
//! Os dois enums são **exaustivos** de propósito: uma variante nova tem de ser tratada por cada
//! backend, e é o compilador que aponta onde.

use serde::{Deserialize, Serialize};

use super::{Button, HidUsage, PointerPosition, WheelDelta};

/// Uma entrada a injetar na máquina local.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Injection {
    /// Uma tecla física, por HID Usage.
    Key {
        /// Qual.
        usage: HidUsage,
        /// `true` para pressionar.
        pressed: bool,
    },
    /// Um botão do ponteiro.
    Button {
        /// Qual.
        button: Button,
        /// `true` para pressionar.
        pressed: bool,
    },
    /// Movimento de roda.
    Wheel(WheelDelta),
    /// Onde o ponteiro deve estar.
    ///
    /// Sempre absoluto, nunca relativo — `docs/05-windows.md` §4.2: injetar movimento relativo
    /// faria o sistema aplicar a própria aceleração a deltas que já vêm acelerados.
    ///
    /// A posição é a do protocolo: uma fração **dentro de um monitor**, o do campo `monitor`, no
    /// arranjo local. Quem a converte no referencial do sistema — o desktop virtual inteiro — é o
    /// injetor, com o arranjo que lhe foi dado.
    Pointer(PointerPosition),
}

/// Um evento capturado da entrada local.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Capture {
    /// O ponteiro se moveu, em deltas relativos. Enviado enquanto o controle está no par: o
    /// cursor local fica preso, e o que interessa é só o quanto ele tentou andar.
    PointerMotion {
        /// Deslocamento horizontal.
        dx: i32,
        /// Deslocamento vertical.
        dy: i32,
    },
    /// O ponteiro está nesta posição absoluta de tela. Enviado enquanto o controle é local.
    ///
    /// Absoluta, e não relativa, porque a sessão precisa saber onde o cursor realmente está para
    /// disparar a travessia na borda certa; acumular deltas a partir de uma origem arbitrária faria
    /// a borda cair no lugar errado.
    PointerAbsolute {
        /// Posição horizontal, em pixels de tela.
        x: i32,
        /// Posição vertical, em pixels de tela.
        y: i32,
    },
    /// A roda girou.
    Wheel(WheelDelta),
    /// Uma tecla mudou de estado.
    Key {
        /// Qual.
        usage: HidUsage,
        /// `true` para pressionada.
        pressed: bool,
    },
    /// Um botão mudou de estado.
    Button {
        /// Qual.
        button: Button,
        /// `true` para pressionado.
        pressed: bool,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ids::MonitorId;

    fn ida_e_volta<T>(valor: &T) -> T
    where
        T: Serialize + for<'de> Deserialize<'de>,
    {
        let bytes = postcard::to_allocvec(valor).expect("codifica");
        postcard::from_bytes(&bytes).expect("decodifica")
    }

    #[test]
    fn toda_injecao_vai_e_volta_pelo_canal_local() {
        let todas = [
            Injection::Key {
                usage: HidUsage::LEFT_CTRL,
                pressed: true,
            },
            Injection::Button {
                button: Button::Forward,
                pressed: false,
            },
            Injection::Wheel(WheelDelta { dx: -3, dy: 120 }),
            Injection::Pointer(PointerPosition {
                monitor: MonitorId(1),
                x: 0,
                y: u16::MAX,
            }),
        ];
        for injecao in todas {
            assert_eq!(ida_e_volta(&injecao), injecao);
        }
    }

    #[test]
    fn toda_captura_vai_e_volta_pelo_canal_local() {
        let todas = [
            Capture::PointerMotion { dx: -7, dy: 9 },
            Capture::PointerAbsolute { x: -1280, y: 1079 },
            Capture::Wheel(WheelDelta::down()),
            Capture::Key {
                usage: HidUsage(0x04),
                pressed: false,
            },
            Capture::Button {
                button: Button::Left,
                pressed: true,
            },
        ];
        for captura in todas {
            assert_eq!(ida_e_volta(&captura), captura);
        }
    }
}
