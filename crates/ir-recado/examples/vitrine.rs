//! Mostra, de verdade, o que a pessoa vê de uma cópia sem abrir a janela: a notificação do sistema,
//! o ícone da bandeja girando e — no Linux — a barra no ícone do dock; depois o fim que deu certo,
//! e um que não deu.
//!
//! ```text
//! cargo run -p ir-recado --example vitrine
//! ```
//!
//! Serve para olhar a aparência depois de mudar texto, ritmo ou quadros, sem montar duas máquinas.
//! No Linux, rode dentro da sessão gráfica (precisa do barramento da sessão).

#![allow(clippy::print_stdout)]

use std::time::{Duration, Instant};

use ir_ipc::transferencia::{Fase, Motivo, Sentido, Transferencia};
use ir_recado::bandeja::aparencia::Retrato;
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

/// A cópia inteira: anda por oito segundos, chega; depois uma que não atravessa.
fn roteiro() -> Vec<(Duration, Transferencia)> {
    let mut passos = vec![(Duration::ZERO, copia(Fase::Anunciada, 0))];
    for i in 1..=20u64 {
        passos.push((
            Duration::from_millis(400),
            copia(Fase::Andando, i * 2_621_440),
        ));
    }
    let chegou = std::env::temp_dir().join("relatorio-anual.pdf");
    let _ = std::fs::write(&chegou, b"vitrine");
    passos.push((
        Duration::from_millis(400),
        copia(
            Fase::Concluida {
                destino: chegou.to_string_lossy().into_owned(),
            },
            52_428_800,
        ),
    ));
    passos.push((
        Duration::from_secs(8),
        copia(Fase::Parada(Motivo::SemPermissao), 0),
    ));
    passos
}

fn main() {
    let mut mostrador = Mostrador::novo();
    let mut ritmo = Ritmo::novo(Duration::from_secs(1));
    for (espera, passo) in roteiro() {
        mostrador.esperar(espera);
        mostrador.copia(&passo);
        if let Some(vez) = ritmo.vez(&passo, Instant::now()) {
            let recado = Recado::da_copia(&passo);
            println!("{:?}: {} — {}", recado.tom, recado.titulo, recado.corpo);
            mostrador.mostrar(&recado, &passo, vez.primeira);
        }
    }
    mostrador.esperar(Duration::from_secs(10));
}

/// Quem mostra, em cada sistema.
struct Mostrador {
    retrato: Retrato,
    #[cfg(windows)]
    central: Option<ir_recado::central::Notificacoes>,
    #[cfg(windows)]
    bandeja: Option<(tray_icon::TrayIcon, ir_recado::bandeja::icones::Icones)>,
    #[cfg(windows)]
    vitrine: ir_recado::bandeja::Vitrine,
    #[cfg(target_os = "linux")]
    bandeja: Option<ir_recado::linux::bandeja::Bandeja>,
    #[cfg(target_os = "linux")]
    doca: Option<ir_recado::linux::doca::Doca>,
}

impl Mostrador {
    fn novo() -> Self {
        Self {
            retrato: Retrato {
                parado: false,
                atravessando: false,
                copia: None,
                janela_visivel: false,
            },
            #[cfg(windows)]
            central: ir_recado::central::Notificacoes::abrir(),
            #[cfg(windows)]
            bandeja: bandeja_do_windows(),
            #[cfg(windows)]
            vitrine: ir_recado::bandeja::Vitrine::default(),
            #[cfg(target_os = "linux")]
            bandeja: ir_recado::linux::bandeja::Bandeja::abrir(|| println!("clicou no ícone")),
            #[cfg(target_os = "linux")]
            doca: ir_recado::linux::doca::Doca::abrir(),
        }
    }

    /// A cópia andou: o ícone e o dock acompanham, a cada passo.
    fn copia(&mut self, copia: &Transferencia) {
        self.retrato.copia = Some(Tom::da_copia(copia));
        #[cfg(target_os = "linux")]
        {
            if let Some(doca) = &mut self.doca {
                doca.andamento(copia.em_curso().then(|| copia.progresso()));
            }
            if let Some(bandeja) = &self.bandeja {
                bandeja.retratar(self.retrato, &copia.detalhe());
            }
        }
    }

    #[cfg_attr(not(windows), allow(unused_variables, clippy::unused_self))]
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
        // No Linux, só o fim: o andamento é da bandeja e do dock.
        #[cfg(target_os = "linux")]
        ir_recado::linux::avisar(recado);
    }

    /// Espera; no Windows, girando o ícone da bandeja a cada décimo de segundo. No Linux quem gira
    /// é a própria bandeja.
    #[cfg_attr(not(windows), allow(clippy::unused_self))]
    fn esperar(&mut self, quanto: Duration) {
        let fim = Instant::now() + quanto;
        while Instant::now() < fim {
            #[cfg(windows)]
            if let Some((icone, icones)) = &self.bandeja
                && let Some(quadro) = self.vitrine.passo(&self.retrato, Instant::now()).quadro
                && let Some(quadro) = icones.quadro(quadro)
            {
                let _ = icone.set_icon(Some(quadro));
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }
}

#[cfg(windows)]
fn bandeja_do_windows() -> Option<(tray_icon::TrayIcon, ir_recado::bandeja::icones::Icones)> {
    let icones = ir_recado::bandeja::icones::Icones::da_bandeja()?;
    let icone = tray_icon::TrayIconBuilder::new()
        .with_icon(icones.quadro(0)?)
        .with_tooltip("InputRemote — vitrine")
        .build()
        .ok()?;
    Some((icone, icones))
}
