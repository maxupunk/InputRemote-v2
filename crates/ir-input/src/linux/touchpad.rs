//! O touchpad, traduzido para o que a sessão entende: deslocamento, rolagem e clique.
//!
//! Um mouse manda deslocamento (`REL_X`/`REL_Y`); um touchpad manda **a posição do dedo** na
//! superfície (`ABS_X`/`ABS_Y`), e quem transforma isso em ponteiro — com rolagem de dois dedos e o
//! toque leve que vira clique — é o compositor, pela `libinput`. A captura por `evdev` fica abaixo
//! dele, então faz uma versão pequena disso aqui. Sem este módulo o touchpad nem era aberto: num
//! notebook, o ponteiro ia até a borda e nada atravessava (log 48).
//!
//! Sem E/S: recebe eventos e o instante, devolve eventos da sessão. Os testes não precisam de
//! touchpad nenhum.

use std::time::{Duration, Instant};

use ir_proto::input::{Button, WheelDelta};

use crate::CaptureEvent;
use crate::roda::AcumuladorDeRoda;

/// Quanto o ponteiro anda ao passar o dedo devagar pela largura inteira do touchpad, em pixels.
///
/// Devagar: a aceleração (`aceleracao.rs`) vem por cima, e uma passada rápida atravessa a tela.
/// O serviço conduz o cursor (log 50), então esta escala só decide a sensação — não há mais cursor
/// real para o modelo alcançar.
const LARGURA_EM_PIXELS: f32 = 1400.0;

/// Quanto o dedo anda, em pixels, para a rolagem de dois dedos dar uma marcação de roda.
const PIXELS_POR_MARCACAO: f32 = 60.0;

/// O toque mais longo que ainda é clique.
const TOQUE_MAXIMO: Duration = Duration::from_millis(180);

/// O quanto o dedo pode andar, em pixels, num toque que ainda é clique.
const TOQUE_PARADO: f32 = 12.0;

/// Os códigos do `evdev` que o touchpad usa. Números, e não os tipos do crate, para os testes
/// escreverem eventos sem dispositivo.
pub(super) mod codigo {
    /// `ABS_X`.
    pub const ABS_X: u16 = 0x00;
    /// `ABS_Y`.
    pub const ABS_Y: u16 = 0x01;
    /// `BTN_TOUCH`: há dedo na superfície.
    pub const BTN_TOUCH: u16 = 0x14a;
    /// `BTN_TOOL_FINGER`: um dedo.
    pub const BTN_TOOL_FINGER: u16 = 0x145;
    /// `BTN_TOOL_DOUBLETAP`: dois dedos.
    pub const BTN_TOOL_DOUBLETAP: u16 = 0x14d;
    /// `BTN_TOOL_TRIPLETAP`: três dedos.
    pub const BTN_TOOL_TRIPLETAP: u16 = 0x14e;
    /// `BTN_TOOL_QUADTAP`: quatro dedos.
    pub const BTN_TOOL_QUADTAP: u16 = 0x14f;
    /// `BTN_LEFT`: o clique físico da superfície.
    pub const BTN_LEFT: u16 = 0x110;
}

/// Um evento do touchpad, já classificado.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Entrada {
    /// Um eixo absoluto (`EV_ABS`).
    Eixo(u16, i32),
    /// Uma tecla ou botão (`EV_KEY`).
    Tecla(u16, i32),
    /// O fim de um relato (`SYN_REPORT`).
    Fim,
}

/// O estado de um touchpad entre um relato e outro.
#[derive(Debug)]
pub(super) struct Touchpad {
    /// Pixels por unidade do eixo.
    escala: f32,
    /// A posição que chegou neste relato.
    posicao: (Option<i32>, Option<i32>),
    /// A posição do relato anterior, enquanto o dedo não sai.
    anterior: Option<(i32, i32)>,
    /// Quantos dedos estão na superfície.
    dedos: u8,
    /// O máximo de dedos neste toque, para o clique de dois dedos.
    dedos_no_toque: u8,
    /// Quando o dedo encostou, se está encostado.
    encostou: Option<Instant>,
    /// Quanto andou neste toque, em pixels.
    andou: f32,
    /// O que sobrou de fração de pixel e de roda, para o movimento lento não sumir.
    resto: (f32, f32),
    roda: AcumuladorDeRoda,
    /// Se houve clique físico neste toque: aí o toque não é clique de novo.
    clicou: bool,
}

impl Touchpad {
    /// Um touchpad cuja largura, em unidades do eixo X, é esta.
    pub(super) fn novo(largura_do_eixo: i32) -> Self {
        Self::com_escala(largura_do_eixo, LARGURA_EM_PIXELS)
    }

    /// Um touchpad em que a largura inteira anda `pixels`.
    pub(super) fn com_escala(largura_do_eixo: i32, pixels: f32) -> Self {
        #[allow(clippy::cast_precision_loss)]
        let largura = largura_do_eixo.max(1) as f32;
        Self {
            escala: pixels / largura,
            posicao: (None, None),
            anterior: None,
            dedos: 0,
            dedos_no_toque: 0,
            encostou: None,
            andou: 0.0,
            resto: (0.0, 0.0),
            // Uma unidade de roda a cada tantos pixels: `PIXELS_POR_MARCACAO` dão uma marcação.
            roda: AcumuladorDeRoda::novo(PIXELS_POR_MARCACAO / f32::from(WheelDelta::NOTCH)),
            clicou: false,
        }
    }

    /// Um evento do touchpad. `None` quando não é dele — um clique físico segue o caminho comum.
    pub(super) fn evento(&mut self, entrada: Entrada, agora: Instant) -> Option<Vec<CaptureEvent>> {
        match entrada {
            Entrada::Eixo(codigo::ABS_X, valor) => self.posicao.0 = Some(valor),
            Entrada::Eixo(codigo::ABS_Y, valor) => self.posicao.1 = Some(valor),
            Entrada::Eixo(..) => {}
            Entrada::Tecla(codigo::BTN_TOUCH, 1) => self.encostar(agora),
            Entrada::Tecla(codigo::BTN_TOUCH, 0) => return Some(self.soltar(agora)),
            Entrada::Tecla(
                tecla @ (codigo::BTN_TOOL_FINGER
                | codigo::BTN_TOOL_DOUBLETAP
                | codigo::BTN_TOOL_TRIPLETAP
                | codigo::BTN_TOOL_QUADTAP),
                1,
            ) => self.contar_dedos(tecla),
            Entrada::Tecla(codigo::BTN_LEFT, _) => {
                self.clicou = true;
                return None;
            }
            Entrada::Tecla(..) => return None,
            Entrada::Fim => return Some(self.relato()),
        }
        Some(Vec::new())
    }

    fn encostar(&mut self, agora: Instant) {
        self.encostou = Some(agora);
        self.anterior = None;
        self.andou = 0.0;
        // A contagem deste toque chega junto, logo depois; a de antes é do toque anterior.
        self.dedos_no_toque = 0;
        self.clicou = false;
    }

    /// O dedo saiu. Um toque curto e parado é clique: um dedo, esquerdo; dois, direito.
    fn soltar(&mut self, agora: Instant) -> Vec<CaptureEvent> {
        let toque = self.encostou.take();
        self.anterior = None;
        self.dedos = 0;
        let foi_clique = toque.is_some_and(|quando| agora.duration_since(quando) <= TOQUE_MAXIMO)
            && self.andou <= TOQUE_PARADO
            && !self.clicou;
        if !foi_clique {
            return Vec::new();
        }
        let button = if self.dedos_no_toque >= 2 {
            Button::Right
        } else {
            Button::Left
        };
        vec![
            CaptureEvent::Button {
                button,
                pressed: true,
            },
            CaptureEvent::Button {
                button,
                pressed: false,
            },
        ]
    }

    /// Mudou o número de dedos. A posição passa a ser de outro dedo, então o próximo relato
    /// recomeça dela, em vez de virar um salto do ponteiro.
    fn contar_dedos(&mut self, tecla: u16) {
        self.dedos = match tecla {
            codigo::BTN_TOOL_DOUBLETAP => 2,
            codigo::BTN_TOOL_TRIPLETAP => 3,
            codigo::BTN_TOOL_QUADTAP => 4,
            _ => 1,
        };
        self.dedos_no_toque = self.dedos_no_toque.max(self.dedos);
        self.anterior = None;
    }

    /// O fim de um relato: o deslocamento desde o anterior, como ponteiro ou como rolagem.
    fn relato(&mut self) -> Vec<CaptureEvent> {
        let (Some(x), Some(y)) = (self.posicao.0, self.posicao.1) else {
            return Vec::new();
        };
        if self.encostou.is_none() {
            return Vec::new();
        }
        let Some((ax, ay)) = self.anterior.replace((x, y)) else {
            return Vec::new();
        };
        #[allow(clippy::cast_precision_loss)]
        let (dx, dy) = ((x - ax) as f32 * self.escala, (y - ay) as f32 * self.escala);
        self.andou += dx.abs() + dy.abs();
        if self.dedos >= 2 {
            return self.rolar(dy);
        }
        self.resto.0 += dx;
        self.resto.1 += dy;
        let (inteiro_x, inteiro_y) = (self.resto.0.trunc(), self.resto.1.trunc());
        self.resto.0 -= inteiro_x;
        self.resto.1 -= inteiro_y;
        #[allow(clippy::cast_possible_truncation)]
        let (dx, dy) = (inteiro_x as i32, inteiro_y as i32);
        if (dx, dy) == (0, 0) {
            return Vec::new();
        }
        vec![CaptureEvent::PointerMotion { dx, dy }]
    }

    /// Dois dedos: rolagem natural, o conteúdo acompanha o dedo — como o GNOME e o Windows fazem no
    /// touchpad por padrão.
    fn rolar(&mut self, dy: f32) -> Vec<CaptureEvent> {
        let (_, unidades) = self.roda.acumular(0.0, dy);
        let dy = i16::try_from(unidades.clamp(i16::MIN.into(), i16::MAX.into())).unwrap_or(0);
        if dy == 0 {
            return Vec::new();
        }
        vec![CaptureEvent::Wheel(WheelDelta { dx: 0, dy })]
    }
}

/// Um touchpad, se este dispositivo for um: aponta (e não é tela de toque), tem posição absoluta
/// e sabe quando há dedo.
pub(super) fn de(dispositivo: &evdev::Device) -> Option<Touchpad> {
    use evdev::{AbsoluteAxisType, Key, PropType};
    let aponta = dispositivo.properties().contains(PropType::POINTER);
    let posicao = dispositivo.supported_absolute_axes().is_some_and(|eixos| {
        eixos.contains(AbsoluteAxisType::ABS_X) && eixos.contains(AbsoluteAxisType::ABS_Y)
    });
    let toque = dispositivo
        .supported_keys()
        .is_some_and(|teclas| teclas.contains(Key::BTN_TOUCH));
    if !(aponta && posicao && toque) {
        return None;
    }
    let eixos = dispositivo.get_abs_state().ok()?;
    let x = eixos.get(usize::from(AbsoluteAxisType::ABS_X.0))?;
    Some(Touchpad::novo(x.maximum.saturating_sub(x.minimum)))
}

/// Um evento do `evdev` na forma que o touchpad lê, se for de um tipo que ele lê.
pub(super) fn entrada(tipo: evdev::InputEventKind, valor: i32) -> Option<Entrada> {
    use evdev::{InputEventKind, Synchronization};
    match tipo {
        InputEventKind::AbsAxis(eixo) => Some(Entrada::Eixo(eixo.0, valor)),
        InputEventKind::Key(tecla) => Some(Entrada::Tecla(tecla.code(), valor)),
        InputEventKind::Synchronization(Synchronization::SYN_REPORT) => Some(Entrada::Fim),
        _ => None,
    }
}

#[cfg(test)]
mod tests;
