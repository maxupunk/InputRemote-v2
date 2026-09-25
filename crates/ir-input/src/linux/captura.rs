//! Captura no Linux por `evdev`: o serviço lê o teclado, o mouse e o touchpad físicos, e os toma
//! para si (`EVIOCGRAB`).
//!
//! # Dois modos de tomar
//!
//! - **Com o controle no par**, tudo é tomado: o compositor para de ver qualquer coisa, e o que se
//!   digita e move vai só para a sessão — é a supressão.
//! - **Com o controle aqui**, o serviço pode **conduzir o cursor** ([`Capturer::conduzir_o_cursor`]):
//!   o mouse e o touchpad continuam tomados, e quem move o cursor do compositor é o serviço, pelo
//!   ponteiro virtual absoluto. O teclado fica com o compositor.
//!
//! O segundo modo existe porque o Wayland não diz a ninguém onde o cursor está. Ouvindo os
//! deslocamentos crus enquanto o compositor movia o cursor com a aceleração dele, o modelo da sessão
//! e o cursor real andavam em velocidades diferentes, e a travessia vinha no meio da tela — com o
//! touchpad e com um mouse USB, antes e depois de reconectar (log 50). Conduzindo, o cursor real
//! **é** o modelo: a travessia acontece exatamente na borda, e a volta põe o cursor onde deve.
//!
//! A condução tem prazo: sem renovação por [`VALIDADE_DA_CONDUCAO`], os dispositivos voltam ao
//! compositor sozinhos. Um serviço travado nunca deixa a máquina sem mouse.
//!
//! Dispositivos novos — um mouse USB espetado depois — entram sozinhos: a lista é refeita a cada
//! [`RELER_A_CADA`]. Os dispositivos virtuais do próprio produto ficam de fora, senão o que o
//! serviço injeta voltaria como captura.

#![allow(unreachable_pub)]
#![allow(unsafe_code)]

use std::collections::HashSet;
use std::os::fd::AsRawFd;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use evdev::{Device, Key, RelativeAxisType};

use super::touchpad;
use super::traducao::Traducao;
use crate::error::{InputError, Result};
use crate::{CaptureEvent, Capturer};

/// De quanto em quanto tempo se procura dispositivo novo.
const RELER_A_CADA: Duration = Duration::from_secs(2);

/// Quanto uma thread de dispositivo espera por evento antes de conferir se deve tomar ou soltar.
const ESPERA: i32 = 20;

/// Quanto vale uma renovação da condução do cursor.
const VALIDADE_DA_CONDUCAO: Duration = Duration::from_secs(3);

/// O que as threads de dispositivo compartilham com quem controla a captura.
#[derive(Debug)]
struct Controle {
    /// Se tudo deve estar tomado (o controle está no par).
    suprimir: AtomicBool,
    /// Até quando, em ms desde [`Self::origem`], o serviço conduz o cursor. `0`: não conduz.
    conduzir_ate: AtomicU64,
    origem: Instant,
    /// Um `eventfd` por thread de dispositivo, para acordá-la na hora quando algo muda.
    despertadores: Mutex<Vec<i32>>,
}

impl Controle {
    fn agora_ms(&self) -> u64 {
        u64::try_from(self.origem.elapsed().as_millis()).unwrap_or(u64::MAX)
    }

    /// Se um dispositivo desta classe deve estar tomado agora.
    fn quer_tomar(&self, classe: Classe) -> bool {
        self.suprimir.load(Ordering::SeqCst)
            || (classe == Classe::Ponteiro
                && self.agora_ms() < self.conduzir_ate.load(Ordering::SeqCst))
    }

    /// Acorda as threads, para a mudança valer na hora e não no próximo evento.
    fn acordar(&self) {
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
}

/// O que um dispositivo é, para decidir quando tomá-lo.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Classe {
    /// Tem teclas de letra — um teclado, ou um combinado com mouse. Só é tomado na supressão:
    /// o serviço não repõe teclas no compositor.
    Teclado,
    /// Mouse ou touchpad, sem teclas de letra. Tomado também enquanto o serviço conduz o cursor.
    Ponteiro,
}

/// A captura por `evdev`.
pub struct EvdevCapturer {
    controle: Arc<Controle>,
}

impl EvdevCapturer {
    /// Começa a capturar todo teclado, mouse e touchpad físico, mandando os eventos para `sink`.
    ///
    /// # Errors
    ///
    /// [`InputError::Unsupported`] se não houver dispositivo legível — sem privilégio de root, ou
    /// numa máquina sem teclado nem mouse.
    pub fn start(sink: Sender<CaptureEvent>) -> Result<Self> {
        let controle = Arc::new(Controle {
            suprimir: AtomicBool::new(false),
            conduzir_ate: AtomicU64::new(0),
            origem: Instant::now(),
            despertadores: Mutex::new(Vec::new()),
        });
        let vistos = Arc::new(Mutex::new(HashSet::new()));
        if abrir_os_novos(&sink, &controle, &vistos) == 0 {
            return Err(InputError::Unsupported);
        }
        let c = Arc::clone(&controle);
        std::thread::Builder::new()
            .name("evdev-procura".to_owned())
            .spawn(move || {
                loop {
                    std::thread::sleep(RELER_A_CADA);
                    let _ = abrir_os_novos(&sink, &c, &vistos);
                }
            })
            .map_err(|erro| InputError::Io(erro.to_string()))?;
        Ok(Self { controle })
    }
}

impl Capturer for EvdevCapturer {
    fn set_suppress(&self, on: bool) {
        self.controle.suprimir.store(on, Ordering::SeqCst);
        self.controle.acordar();
    }

    fn warp_pointer(&self, _x: i32, _y: i32) {
        // O cursor se move pelo ponteiro virtual, quando o serviço o conduz; `evdev` só lê.
    }

    fn conduzir_o_cursor(&self, on: bool) {
        let ate = if on {
            let validade = u64::try_from(VALIDADE_DA_CONDUCAO.as_millis()).unwrap_or(0);
            self.controle.agora_ms().saturating_add(validade).max(1)
        } else {
            0
        };
        let antes = self.controle.conduzir_ate.swap(ate, Ordering::SeqCst);
        // Só acorda quando liga ou desliga; a renovação de cada segundo não muda nada nas threads.
        if (antes == 0) != (ate == 0) {
            self.controle.acordar();
        }
    }
}

/// Abre os dispositivos de entrada ainda não vistos. Devolve quantos estão sendo lidos agora.
fn abrir_os_novos(
    sink: &Sender<CaptureEvent>,
    controle: &Arc<Controle>,
    vistos: &Arc<Mutex<HashSet<PathBuf>>>,
) -> usize {
    let Ok(mut ja_vistos) = vistos.lock() else {
        return 0;
    };
    for (caminho, dispositivo) in evdev::enumerate() {
        if ja_vistos.contains(&caminho) {
            continue;
        }
        let Some(classe) = classe(&dispositivo) else {
            continue;
        };
        // SAFETY: `eventfd` não recebe ponteiro; o descritor é fechado pela thread ao sair.
        let despertador = unsafe { libc::eventfd(0, libc::EFD_NONBLOCK | libc::EFD_CLOEXEC) };
        if despertador < 0 {
            continue;
        }
        if let Ok(mut lista) = controle.despertadores.lock() {
            lista.push(despertador);
        }
        ja_vistos.insert(caminho.clone());
        let (sink, c, v) = (sink.clone(), Arc::clone(controle), Arc::clone(vistos));
        let _ = std::thread::Builder::new()
            .name("evdev".to_owned())
            .spawn(move || {
                ler(dispositivo, classe, despertador, &sink, &c);
                // O dispositivo sumiu: sai da lista, e volta se reaparecer.
                if let Ok(mut lista) = c.despertadores.lock() {
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

/// O que o dispositivo é, se for teclado, mouse ou touchpad físico — e não um virtual do produto.
fn classe(dispositivo: &Device) -> Option<Classe> {
    if dispositivo.name().is_some_and(super::e_virtual_do_produto) {
        return None;
    }
    let teclado = dispositivo
        .supported_keys()
        .is_some_and(|teclas| teclas.contains(Key::KEY_A) && teclas.contains(Key::KEY_ENTER));
    let mouse = dispositivo.supported_relative_axes().is_some_and(|eixos| {
        eixos.contains(RelativeAxisType::REL_X) && eixos.contains(RelativeAxisType::REL_Y)
    });
    if teclado {
        Some(Classe::Teclado)
    } else if mouse || touchpad::de(dispositivo).is_some() {
        // O touchpad do notebook manda posição, e não deslocamento (log 48).
        Some(Classe::Ponteiro)
    } else {
        None
    }
}

/// Lê um dispositivo até ele sumir, tomando-o e soltando-o conforme o [`Controle`].
fn ler(
    mut dispositivo: Device,
    classe: Classe,
    despertador: i32,
    sink: &Sender<CaptureEvent>,
    controle: &Controle,
) {
    let mut tomado = false;
    let mut traducao = Traducao::nova(touchpad::de(&dispositivo));
    loop {
        let quer = controle.quer_tomar(classe);
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
        match esperar(&dispositivo, despertador) {
            Espera::Nada => continue,
            Espera::Sumiu => return,
            Espera::Eventos => {}
        }
        let Ok(eventos) = dispositivo.fetch_events() else {
            return;
        };
        for evento in eventos {
            if traducao
                .de(&evento)
                .into_iter()
                .any(|c| sink.send(c).is_err())
            {
                return;
            }
        }
    }
}

/// O resultado de uma espera por eventos.
enum Espera {
    Nada,
    Sumiu,
    Eventos,
}

/// Espera evento do dispositivo ou um despertar, por no máximo [`ESPERA`] ms.
fn esperar(dispositivo: &Device, despertador: i32) -> Espera {
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
    if unsafe { libc::poll(espera.as_mut_ptr(), 2, ESPERA) } < 0 {
        return Espera::Sumiu;
    }
    if espera[1].revents & libc::POLLIN != 0 {
        let mut lixo = 0u64;
        // SAFETY: lê os 8 bytes do contador do `eventfd` para zerá-lo.
        unsafe {
            libc::read(despertador, std::ptr::from_mut(&mut lixo).cast(), 8);
        }
    }
    if espera[0].revents & (libc::POLLERR | libc::POLLHUP) != 0 {
        return Espera::Sumiu;
    }
    if espera[0].revents & libc::POLLIN == 0 {
        Espera::Nada
    } else {
        Espera::Eventos
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn controle() -> Controle {
        Controle {
            suprimir: AtomicBool::new(false),
            conduzir_ate: AtomicU64::new(0),
            origem: Instant::now(),
            despertadores: Mutex::new(Vec::new()),
        }
    }

    #[test]
    fn na_supressao_tudo_e_tomado() {
        let c = controle();
        c.suprimir.store(true, Ordering::SeqCst);
        assert!(c.quer_tomar(Classe::Teclado));
        assert!(c.quer_tomar(Classe::Ponteiro));
    }

    #[test]
    fn conduzindo_so_o_ponteiro_e_tomado_e_o_teclado_fica_com_o_compositor() {
        let c = controle();
        assert!(
            !c.quer_tomar(Classe::Ponteiro),
            "sem condução, nada é tomado"
        );
        c.conduzir_ate.store(c.agora_ms() + 3000, Ordering::SeqCst);
        assert!(c.quer_tomar(Classe::Ponteiro));
        assert!(!c.quer_tomar(Classe::Teclado));
    }

    #[test]
    fn a_conducao_vencida_devolve_o_mouse_sozinha() {
        // Um serviço travado não renova: o mouse volta ao compositor.
        let c = controle();
        c.conduzir_ate.store(1, Ordering::SeqCst);
        std::thread::sleep(Duration::from_millis(5));
        assert!(!c.quer_tomar(Classe::Ponteiro));
    }
}
