//! O ícone na bandeja do Linux: o `StatusNotifierItem`, o padrão que o KDE mostra sozinho e o
//! GNOME mostra com a extensão `AppIndicator` (no Ubuntu, ligada de fábrica).
//!
//! O mesmo ícone do Windows — gira enquanto algo atravessa, mostra o ✓ ou o ! no fim de uma cópia,
//! fica cinza sem o outro computador —, com a mesma decisão ([`crate::bandeja::Vitrine`]) e os
//! mesmos quadros. Muda o compasso: um quadro a cada 250 ms, e não a cada 100. Cada quadro novo é
//! um sinal no barramento que o painel redesenha, e o GNOME Shell é um processo só para tudo.
//!
//! **O ! some quando a pessoa mexe no ícone.** No Windows ele some quando a janela abre; aqui a
//! janela é outro processo, e quase sempre está fechada. Clicar no ícone, ou abrir o menu dele, é
//! o "vi" — e o menu mostra, no alto, a frase do que houve.
//!
//! **Sem quem mostre, espera.** Com a extensão desligada não há `StatusNotifierWatcher`; o ícone
//! fica pronto e aparece sozinho no instante em que alguém liga a extensão — sem reiniciar nada.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use ksni::blocking::TrayMethods as _;

use crate::bandeja::Vitrine;
use crate::bandeja::aparencia::Retrato;
use crate::bandeja::quadros;

/// O compasso do ícone.
const BATIDA: Duration = Duration::from_millis(250);

/// O que a bandeja mostra agora: o que acontece, e a frase da dica.
type Agora = (Retrato, String);

/// O ícone na bandeja. Some quando este valor cai.
#[derive(Debug)]
pub struct Bandeja {
    agora: Arc<Mutex<Agora>>,
    /// Fechado quando a bandeja cai, e a batida termina.
    _fim: mpsc::Sender<()>,
}

impl Bandeja {
    /// Põe o ícone e começa a batida. `abrir` é o que o clique e o menu fazem: abrir a janela.
    ///
    /// `None` sem barramento da sessão, ou sem os quadros.
    pub fn abrir(abrir: impl Fn() + Send + Sync + 'static) -> Option<Self> {
        let quadros = Arc::new(quadros_argb()?);
        let visto = Arc::new(AtomicBool::new(false));
        let item = Item {
            quadro: 0,
            dica: String::new(),
            quadros,
            abrir: Arc::new(abrir),
            visto: Arc::clone(&visto),
        };
        let ponteiro = item.assume_sni_available(true).spawn().ok()?;
        let agora = Arc::new(Mutex::new((
            Retrato {
                parado: true,
                atravessando: false,
                copia: None,
                janela_visivel: false,
            },
            String::new(),
        )));
        let (fim, acabou) = mpsc::channel();
        let compartilhado = Arc::clone(&agora);
        std::thread::Builder::new()
            .name("bandeja".to_owned())
            .spawn(move || bater(&ponteiro, &compartilhado, &visto, &acabou))
            .ok()?;
        Some(Self { agora, _fim: fim })
    }

    /// O que está acontecendo agora, e a frase da dica.
    pub fn retratar(&self, retrato: Retrato, dica: &str) {
        let mut agora = self.agora.lock().unwrap_or_else(PoisonError::into_inner);
        *agora = (retrato, dica.to_owned());
    }
}

/// A batida: o quadro e a dica, a cada compasso, até a bandeja cair.
fn bater(
    ponteiro: &ksni::blocking::Handle<Item>,
    agora: &Mutex<Agora>,
    visto: &AtomicBool,
    acabou: &mpsc::Receiver<()>,
) {
    let mut vitrine = Vitrine::default();
    let mut dica_posta = None;
    while let Err(RecvTimeoutError::Timeout) = acabou.recv_timeout(BATIDA) {
        let (mut retrato, dica) = agora.lock().unwrap_or_else(PoisonError::into_inner).clone();
        retrato.janela_visivel = visto.swap(false, Ordering::Relaxed);
        let quadro = vitrine.passo(&retrato, Instant::now()).quadro;
        let dica_nova = (dica_posta.as_ref() != Some(&dica)).then_some(dica);
        if quadro.is_none() && dica_nova.is_none() {
            continue;
        }
        ponteiro.update(|item| {
            if let Some(quadro) = quadro {
                item.quadro = quadro;
            }
            if let Some(dica) = &dica_nova {
                item.dica.clone_from(dica);
            }
        });
        if dica_nova.is_some() {
            dica_posta = dica_nova;
        }
    }
    ponteiro.shutdown().wait();
}

/// Os quadros de todos os tamanhos, em ARGB, como o `StatusNotifierItem` pede: `[quadro][tamanho]`.
fn quadros_argb() -> Option<Vec<Vec<ksni::Icon>>> {
    let tamanhos = quadros::de_todos_os_tamanhos();
    if tamanhos.is_empty() {
        return None;
    }
    Some(
        (0..quadros::QUADROS)
            .map(|indice| {
                tamanhos
                    .iter()
                    .filter_map(|tira| tira.get(indice))
                    .map(|quadro| ksni::Icon {
                        width: i32::try_from(quadro.lado).unwrap_or(16),
                        height: i32::try_from(quadro.lado).unwrap_or(16),
                        data: argb(&quadro.rgba),
                    })
                    .collect()
            })
            .collect(),
    )
}

/// RGBA para ARGB, a ordem do `StatusNotifierItem`.
fn argb(rgba: &[u8]) -> Vec<u8> {
    rgba.chunks_exact(4)
        .flat_map(|pixel| match *pixel {
            [r, g, b, a] => [a, r, g, b],
            _ => [0; 4],
        })
        .collect()
}

/// O ícone, como o painel o pergunta.
struct Item {
    quadro: usize,
    dica: String,
    quadros: Arc<Vec<Vec<ksni::Icon>>>,
    abrir: Arc<dyn Fn() + Send + Sync>,
    /// A pessoa mexeu no ícone: o que ele contava foi visto.
    visto: Arc<AtomicBool>,
}

impl ksni::Tray for Item {
    fn id(&self) -> String {
        "inputremote".to_owned()
    }

    fn title(&self) -> String {
        "InputRemote".to_owned()
    }

    fn icon_pixmap(&self) -> Vec<ksni::Icon> {
        self.quadros.get(self.quadro).cloned().unwrap_or_default()
    }

    fn tool_tip(&self) -> ksni::ToolTip {
        ksni::ToolTip {
            title: "InputRemote".to_owned(),
            description: self.dica.clone(),
            icon_name: String::new(),
            icon_pixmap: Vec::new(),
        }
    }

    fn activate(&mut self, _x: i32, _y: i32) {
        self.visto.store(true, Ordering::Relaxed);
        (self.abrir)();
    }

    fn menu_about_to_show(&mut self) {
        self.visto.store(true, Ordering::Relaxed);
    }

    fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
        // A dica numa linha apagada, no alto: no GNOME o menu é onde se lê o que está acontecendo.
        let mut menu = Vec::new();
        if !self.dica.is_empty() {
            menu.push(
                ksni::menu::StandardItem {
                    label: self.dica.replace('_', "__"),
                    enabled: false,
                    ..Default::default()
                }
                .into(),
            );
            menu.push(ksni::MenuItem::Separator);
        }
        menu.push(
            ksni::menu::StandardItem {
                label: "Abrir o InputRemote".to_owned(),
                activate: Box::new(|item: &mut Self| (item.abrir)()),
                ..Default::default()
            }
            .into(),
        );
        menu
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn os_quadros_vao_em_argb_com_todos_os_tamanhos() {
        assert_eq!(argb(&[1, 2, 3, 4, 5, 6, 7, 8]), [4, 1, 2, 3, 8, 5, 6, 7]);
        let quadros = quadros_argb().expect("os quadros abrem");
        assert_eq!(quadros.len(), quadros::QUADROS);
        assert!(quadros.iter().all(|tamanhos| tamanhos.len() == 4));
    }
}
