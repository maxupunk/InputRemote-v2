//! Mostra, de verdade, o que a pessoa vê de uma cópia sem abrir a janela: a notificação do sistema
//! com a barra andando e o ícone da bandeja girando; depois o fim que deu certo, e um que não deu.
//!
//! ```text
//! cargo run -p ir-recado --example vitrine
//! ```
//!
//! No Windows, a central de notificações e a bandeja; no Linux, o `notify-send`. Serve para olhar a
//! aparência depois de mudar texto, ritmo ou quadros, sem montar duas máquinas.

#![allow(clippy::print_stdout)]

use std::time::{Duration, Instant};

use ir_ipc::transferencia::{Fase, Motivo, Sentido, Transferencia};
use ir_recado::{Recado, Ritmo, Tom};

fn copia(fase: Fase, feitos: u64) -> Transferencia {
    Transferencia {
        sentido: Sentido::Recebendo,
        nome: "relatorio-anual.pdf".to_owned(),
        bytes_feitos: feitos,
        bytes_total: 52_428_800,
        fase,
    }
}

/// A cópia inteira: anda por quatro segundos, chega; depois uma que não atravessa.
fn roteiro() -> Vec<(Duration, Transferencia)> {
    let mut passos = vec![(Duration::ZERO, copia(Fase::Anunciada, 0))];
    for i in 1..=20u64 {
        passos.push((
            Duration::from_millis(200),
            copia(Fase::Andando, i * 2_621_440),
        ));
    }
    let pasta = std::env::temp_dir().join("relatorio-anual.pdf");
    passos.push((
        Duration::from_millis(200),
        copia(
            Fase::Concluida {
                destino: pasta.to_string_lossy().into_owned(),
            },
            52_428_800,
        ),
    ));
    passos.push((
        Duration::from_secs(5),
        copia(Fase::Parada(Motivo::SemPermissao), 0),
    ));
    passos
}

fn main() {
    let mut mostrador = Mostrador::novo();
    let mut ritmo = Ritmo::novo(Duration::from_secs(1));
    for (espera, passo) in roteiro() {
        mostrador.esperar(espera);
        if let Some(vez) = ritmo.vez(&passo, Instant::now()) {
            let recado = Recado::da_copia(&passo);
            println!("{:?}: {} — {}", recado.tom, recado.titulo, recado.corpo);
            mostrador.mostrar(&recado, &passo, vez.primeira);
        }
        mostrador.tom = Some(Recado::da_copia(&passo).tom);
    }
    mostrador.esperar(Duration::from_secs(6));
}

/// Quem mostra, em cada sistema.
struct Mostrador {
    tom: Option<Tom>,
    #[cfg(windows)]
    central: Option<ir_recado::central::Notificacoes>,
    #[cfg(windows)]
    bandeja: Option<Bandeja>,
    #[cfg(target_os = "linux")]
    notify_send: ir_recado::linux::NotifySend,
}

#[cfg(windows)]
struct Bandeja {
    icone: tray_icon::TrayIcon,
    icones: ir_recado::bandeja::icones::Icones,
    selo: ir_recado::bandeja::aparencia::Selo,
    batida: usize,
}

impl Mostrador {
    fn novo() -> Self {
        Self {
            tom: None,
            #[cfg(windows)]
            central: ir_recado::central::Notificacoes::abrir(),
            #[cfg(windows)]
            bandeja: Bandeja::nova(),
            #[cfg(target_os = "linux")]
            notify_send: ir_recado::linux::NotifySend::default(),
        }
    }

    #[cfg_attr(not(windows), allow(unused_variables))]
    fn mostrar(&mut self, recado: &Recado, copia: &Transferencia, primeira: bool) {
        #[cfg(windows)]
        if let Some(central) = &mut self.central {
            let atualizou = copia.em_curso() && !primeira && central.atualizar(recado, "copia");
            if !atualizou {
                central.mostrar(recado, Some("copia"));
            }
            println!(
                "   {}",
                if atualizou {
                    "atualizada no lugar"
                } else {
                    "mostrada"
                }
            );
        }
        #[cfg(target_os = "linux")]
        self.notify_send.mostrar(recado, !primeira);
    }

    /// Espera, girando o ícone da bandeja a cada décimo de segundo. Fora do Windows, só espera.
    #[cfg_attr(not(windows), allow(clippy::unused_self))]
    fn esperar(&mut self, quanto: Duration) {
        let fim = Instant::now() + quanto;
        while Instant::now() < fim {
            #[cfg(windows)]
            if let Some(bandeja) = &mut self.bandeja {
                bandeja.batida(self.tom);
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }
}

#[cfg(windows)]
impl Bandeja {
    fn nova() -> Option<Self> {
        let icones = ir_recado::bandeja::icones::Icones::da_bandeja()?;
        let primeiro = icones.quadro(ir_recado::bandeja::aparencia::Aparencia::Normal, 0)?;
        let icone = tray_icon::TrayIconBuilder::new()
            .with_icon(primeiro)
            .with_tooltip("InputRemote — vitrine")
            .build()
            .ok()?;
        Some(Self {
            icone,
            icones,
            selo: ir_recado::bandeja::aparencia::Selo::default(),
            batida: 0,
        })
    }

    fn batida(&mut self, tom: Option<Tom>) {
        let retrato = ir_recado::bandeja::aparencia::Retrato {
            parado: false,
            atravessando: false,
            copia: tom,
            janela_visivel: false,
        };
        let aparencia = self.selo.aparencia(&retrato, Instant::now());
        self.batida += 1;
        if let Some(quadro) = self.icones.quadro(aparencia, self.batida) {
            let _ = self.icone.set_icon(Some(quadro));
        }
    }
}
