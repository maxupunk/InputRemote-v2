//! Captura no Linux por `evdev`: o serviço lê o teclado e o mouse físicos, e os toma para si
//! (`EVIOCGRAB`) enquanto o controle está no par.
//!
//! É o "modo `evdev` exclusivo" de [06, §3.3](../../../docs/06-linux.md), numa forma sem curva de
//! aceleração: com o controle aqui, os dispositivos continuam entregando ao compositor, e o
//! serviço só **ouve** os deltas crus para o modelo de ponteiro da sessão. Ao atravessar, o
//! serviço toma os dispositivos, e o compositor para de ver qualquer coisa — é a supressão.
//!
//! O que isto não resolve, e por que há o atalho: sem saber onde o compositor pôs o cursor, o
//! modelo da sessão diverge dele pela aceleração, e a travessia pela borda acontece perto do ponto
//! certo, não exatamente nele. Ctrl+Alt+Shift+Espaço atravessa na hora, sem borda nenhuma. O
//! caminho exato continua sendo o portal `InputCapture` ([06, §3.1](../../../docs/06-linux.md)).
//!
//! Dispositivos novos — um teclado USB espetado depois — entram sozinhos: a lista é refeita a cada
//! [`RELER_A_CADA`]. Os dispositivos virtuais do próprio produto ficam de fora, senão o que o
//! serviço injeta como cliente voltaria como captura.

#![allow(unreachable_pub)]
#![allow(unsafe_code)]

use std::collections::HashSet;
use std::os::fd::AsRawFd;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use evdev::{Device, InputEventKind, Key, RelativeAxisType};
use ir_proto::input::{Button, WheelDelta};

use crate::error::{InputError, Result};
use crate::{CaptureEvent, Capturer};

/// De quanto em quanto tempo se procura dispositivo novo.
const RELER_A_CADA: Duration = Duration::from_secs(2);

/// Quanto uma thread de dispositivo espera por evento antes de conferir se deve tomar ou soltar.
const ESPERA: i32 = 20;

/// A captura por `evdev`.
pub struct EvdevCapturer {
    /// Se os dispositivos devem estar tomados (supressão ligada).
    suprimir: Arc<AtomicBool>,
    /// Um `eventfd` por thread de dispositivo, para acordá-la na hora quando a supressão muda.
    despertadores: Arc<Mutex<Vec<i32>>>,
}

impl EvdevCapturer {
    /// Começa a capturar todo teclado e mouse físico, mandando os eventos para `sink`.
    ///
    /// # Errors
    ///
    /// [`InputError::Unsupported`] se não houver dispositivo legível — sem privilégio de root, ou
    /// numa máquina sem teclado nem mouse.
    pub fn start(sink: Sender<CaptureEvent>) -> Result<Self> {
        let suprimir = Arc::new(AtomicBool::new(false));
        let despertadores = Arc::new(Mutex::new(Vec::new()));
        let vistos = Arc::new(Mutex::new(HashSet::new()));
        let abertos = abrir_os_novos(&sink, &suprimir, &despertadores, &vistos);
        if abertos == 0 {
            return Err(InputError::Unsupported);
        }
        let (s, d, v) = (Arc::clone(&suprimir), Arc::clone(&despertadores), vistos);
        std::thread::Builder::new()
            .name("evdev-procura".to_owned())
            .spawn(move || {
                loop {
                    std::thread::sleep(RELER_A_CADA);
                    let _ = abrir_os_novos(&sink, &s, &d, &v);
                }
            })
            .map_err(|erro| InputError::Io(erro.to_string()))?;
        Ok(Self {
            suprimir,
            despertadores,
        })
    }
}

impl Capturer for EvdevCapturer {
    fn set_suppress(&self, on: bool) {
        self.suprimir.store(on, Ordering::SeqCst);
        if let Ok(despertadores) = self.despertadores.lock() {
            for &fd in despertadores.iter() {
                let um: u64 = 1;
                // SAFETY: `fd` é um `eventfd` aberto por uma thread de dispositivo, e escrever 8
                // bytes nele só incrementa o contador.
                unsafe {
                    libc::write(fd, std::ptr::from_ref(&um).cast(), 8);
                }
            }
        }
    }

    fn warp_pointer(&self, _x: i32, _y: i32) {
        // O compositor é quem sabe onde o cursor está, e não há como movê-lo por `evdev`.
    }
}

/// Abre os dispositivos de entrada ainda não vistos. Devolve quantos estão sendo lidos agora.
fn abrir_os_novos(
    sink: &Sender<CaptureEvent>,
    suprimir: &Arc<AtomicBool>,
    despertadores: &Arc<Mutex<Vec<i32>>>,
    vistos: &Arc<Mutex<HashSet<PathBuf>>>,
) -> usize {
    let Ok(mut ja_vistos) = vistos.lock() else {
        return 0;
    };
    for (caminho, dispositivo) in evdev::enumerate() {
        if ja_vistos.contains(&caminho) || !de_entrada(&dispositivo) {
            continue;
        }
        // SAFETY: `eventfd` não recebe ponteiro; o descritor é fechado pela thread ao sair.
        let despertador = unsafe { libc::eventfd(0, libc::EFD_NONBLOCK | libc::EFD_CLOEXEC) };
        if despertador < 0 {
            continue;
        }
        if let Ok(mut lista) = despertadores.lock() {
            lista.push(despertador);
        }
        ja_vistos.insert(caminho.clone());
        let (sink, suprimir) = (sink.clone(), Arc::clone(suprimir));
        let (d, v) = (Arc::clone(despertadores), Arc::clone(vistos));
        let _ = std::thread::Builder::new()
            .name("evdev".to_owned())
            .spawn(move || {
                ler(dispositivo, despertador, &sink, &suprimir);
                // O dispositivo sumiu: sai da lista, e volta se reaparecer.
                if let Ok(mut lista) = d.lock() {
                    lista.retain(|&fd| fd != despertador);
                }
                if let Ok(mut vistos) = v.lock() {
                    vistos.remove(&caminho);
                }
                // SAFETY: o descritor é desta thread, e ninguém mais o usa depois de sair da lista.
                unsafe {
                    libc::close(despertador);
                }
            });
    }
    ja_vistos.len()
}

/// Se o dispositivo é um teclado ou um mouse físico — e não um dos virtuais do produto.
fn de_entrada(dispositivo: &Device) -> bool {
    if dispositivo
        .name()
        .is_some_and(|nome| nome.starts_with("InputRemote"))
    {
        return false;
    }
    let teclado = dispositivo
        .supported_keys()
        .is_some_and(|teclas| teclas.contains(Key::KEY_A) && teclas.contains(Key::KEY_ENTER));
    let mouse = dispositivo.supported_relative_axes().is_some_and(|eixos| {
        eixos.contains(RelativeAxisType::REL_X) && eixos.contains(RelativeAxisType::REL_Y)
    });
    teclado || mouse
}

/// Lê um dispositivo até ele sumir, tomando-o e soltando-o conforme a supressão.
fn ler(
    mut dispositivo: Device,
    despertador: i32,
    sink: &Sender<CaptureEvent>,
    suprimir: &AtomicBool,
) {
    let mut tomado = false;
    let mut movimento = (0i32, 0i32);
    loop {
        let quer = suprimir.load(Ordering::SeqCst);
        if quer != tomado {
            let feito = if quer {
                dispositivo.grab()
            } else {
                dispositivo.ungrab()
            };
            if feito.is_ok() {
                tomado = quer;
            }
        }
        let mut espera = [
            libc::pollfd {
                fd: dispositivo.as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            },
            libc::pollfd {
                fd: despertador,
                events: libc::POLLIN,
                revents: 0,
            },
        ];
        // SAFETY: os dois `pollfd` são válidos e vivem até o fim da chamada.
        let prontos = unsafe { libc::poll(espera.as_mut_ptr(), 2, ESPERA) };
        if prontos < 0 {
            return;
        }
        if espera[1].revents & libc::POLLIN != 0 {
            let mut lixo = 0u64;
            // SAFETY: lê os 8 bytes do contador do `eventfd` para zerá-lo.
            unsafe {
                libc::read(despertador, std::ptr::from_mut(&mut lixo).cast(), 8);
            }
        }
        if espera[0].revents & (libc::POLLERR | libc::POLLHUP) != 0 {
            return; // desconectado
        }
        if espera[0].revents & libc::POLLIN == 0 {
            continue;
        }
        let Ok(eventos) = dispositivo.fetch_events() else {
            return;
        };
        for evento in eventos {
            if let Some(capturado) = traduzir(evento.kind(), evento.value(), &mut movimento)
                && sink.send(capturado).is_err()
            {
                return;
            }
        }
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
