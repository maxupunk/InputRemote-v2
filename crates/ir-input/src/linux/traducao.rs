//! Os eventos do `evdev`, no vocabulário da sessão: deslocamento acelerado, roda, botões e teclas.
//!
//! Separado da captura, que cuida dos dispositivos e de quando tomá-los; aqui é só tradução, com o
//! estado que ela precisa por dispositivo — o movimento do relato em curso, o touchpad e a
//! aceleração.

use std::time::{Instant, UNIX_EPOCH};

use evdev::{InputEvent, InputEventKind, Key, RelativeAxisType};
use ir_proto::input::{Button, WheelDelta};

use super::aceleracao::Acelerador;
use super::touchpad::{self, Touchpad};
use crate::CaptureEvent;

/// O estado de tradução de um dispositivo.
pub(super) struct Traducao {
    movimento: (i32, i32),
    touchpad: Option<Touchpad>,
    acelerador: Acelerador,
}

impl Traducao {
    /// A tradução de um dispositivo; `touchpad` quando ele for um.
    pub(super) fn nova(touchpad: Option<Touchpad>) -> Self {
        Self {
            movimento: (0, 0),
            touchpad,
            acelerador: Acelerador::default(),
        }
    }

    /// O que um evento do `evdev` vira na sessão, já com a aceleração no deslocamento.
    pub(super) fn de(&mut self, evento: &InputEvent) -> Vec<CaptureEvent> {
        let (tipo, valor) = (evento.kind(), evento.value());
        let do_touchpad = self
            .touchpad
            .as_mut()
            .and_then(|t| touchpad::entrada(tipo, valor).and_then(|e| t.evento(e, Instant::now())));
        let capturados = do_touchpad.unwrap_or_else(|| {
            traduzir(tipo, valor, &mut self.movimento)
                .into_iter()
                .collect()
        });
        let quando = evento
            .timestamp()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default();
        capturados
            .into_iter()
            .map(|capturado| match capturado {
                CaptureEvent::PointerMotion { dx, dy } => {
                    let (dx, dy) = self.acelerador.aplicar(dx, dy, quando);
                    CaptureEvent::PointerMotion { dx, dy }
                }
                outro => outro,
            })
            .filter(|c| !matches!(c, CaptureEvent::PointerMotion { dx: 0, dy: 0 }))
            .collect()
    }
}

/// Traduz um evento do `evdev` no que a sessão entende. Movimento se acumula até o fim do relato.
fn traduzir(tipo: InputEventKind, valor: i32, movimento: &mut (i32, i32)) -> Option<CaptureEvent> {
    match tipo {
        InputEventKind::RelAxis(RelativeAxisType::REL_X) => {
            movimento.0 = movimento.0.saturating_add(valor);
            None
        }
        InputEventKind::RelAxis(RelativeAxisType::REL_Y) => {
            movimento.1 = movimento.1.saturating_add(valor);
            None
        }
        InputEventKind::RelAxis(RelativeAxisType::REL_WHEEL) => {
            Some(CaptureEvent::Wheel(WheelDelta {
                dx: 0,
                dy: roda(valor),
            }))
        }
        InputEventKind::RelAxis(RelativeAxisType::REL_HWHEEL) => {
            Some(CaptureEvent::Wheel(WheelDelta {
                dx: roda(valor),
                dy: 0,
            }))
        }
        InputEventKind::Synchronization(_) if *movimento != (0, 0) => {
            let (dx, dy) = core::mem::take(movimento);
            Some(CaptureEvent::PointerMotion { dx, dy })
        }
        // A repetição automática (valor 2) fica com o sistema do outro lado, que repete sozinho a
        // tecla que continua apertada.
        InputEventKind::Key(tecla) if valor == 0 || valor == 1 => {
            let pressed = valor == 1;
            if let Some(button) = botao(tecla) {
                return Some(CaptureEvent::Button { button, pressed });
            }
            super::keymap::key_to_hid(tecla).map(|usage| CaptureEvent::Key { usage, pressed })
        }
        _ => None,
    }
}

/// Uma marcação de roda do `evdev` nas unidades do protocolo.
fn roda(marcacoes: i32) -> i16 {
    i16::try_from(marcacoes.saturating_mul(i32::from(WheelDelta::NOTCH))).unwrap_or(0)
}

/// O botão do mouse que esta tecla do `evdev` é, se for um.
const fn botao(tecla: Key) -> Option<Button> {
    Some(match tecla {
        Key::BTN_LEFT => Button::Left,
        Key::BTN_RIGHT => Button::Right,
        Key::BTN_MIDDLE => Button::Middle,
        Key::BTN_SIDE => Button::Back,
        Key::BTN_EXTRA => Button::Forward,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn o_movimento_sai_inteiro_no_fim_do_relato() {
        let mut movimento = (0, 0);
        assert!(
            traduzir(
                InputEventKind::RelAxis(RelativeAxisType::REL_X),
                3,
                &mut movimento
            )
            .is_none()
        );
        assert!(
            traduzir(
                InputEventKind::RelAxis(RelativeAxisType::REL_Y),
                -2,
                &mut movimento
            )
            .is_none()
        );
        let fim = traduzir(
            InputEventKind::Synchronization(evdev::Synchronization::SYN_REPORT),
            0,
            &mut movimento,
        );
        assert_eq!(fim, Some(CaptureEvent::PointerMotion { dx: 3, dy: -2 }));
        assert_eq!(movimento, (0, 0));
    }

    #[test]
    fn teclas_botoes_e_roda_viram_eventos_da_sessao() {
        let mut m = (0, 0);
        assert!(matches!(
            traduzir(InputEventKind::Key(Key::KEY_A), 1, &mut m),
            Some(CaptureEvent::Key { pressed: true, .. })
        ));
        assert_eq!(
            traduzir(InputEventKind::Key(Key::KEY_A), 2, &mut m),
            None,
            "a repetição automática fica com o outro lado"
        );
        assert_eq!(
            traduzir(InputEventKind::Key(Key::BTN_RIGHT), 0, &mut m),
            Some(CaptureEvent::Button {
                button: Button::Right,
                pressed: false
            })
        );
        assert_eq!(
            traduzir(
                InputEventKind::RelAxis(RelativeAxisType::REL_WHEEL),
                1,
                &mut m
            ),
            Some(CaptureEvent::Wheel(WheelDelta { dx: 0, dy: 120 }))
        );
    }
}
