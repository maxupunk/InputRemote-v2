//! Uma bancada para desenvolver o aplicativo Android sem o serviço instalado.
//!
//! ```text
//! cargo run -p ir-monitor-rede --example bancada
//! ```
//!
//! Sobe o monitor de verdade na porta do painel, com a janela de pareamento aberta, a amostra
//! desta máquina pela coleta de verdade e um par de mentira ao lado — para a tela mostrar os dois
//! cartões e a faixa do enlace. O código de pareamento aparece aqui e é confirmado sozinho: é uma
//! bancada, e quem está nela olha as duas telas.
//!
//! No emulador do Android, o computador é o `10.0.2.2`: digite-o em "IP do computador".

use std::time::{Duration, Instant};

use ir_crypto::Identity;
use ir_monitor::{Mesa, Rota};
use ir_monitor_rede::{Aviso, Comando, Monitor};
use ir_proto::message::UsageSample;
use tokio::sync::mpsc;

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let (avisos_tx, mut avisos) = mpsc::unbounded_channel();
    let monitor = Monitor::iniciar(Identity::generate(), "bancada", Vec::new(), avisos_tx);
    monitor.comandar(Comando::AbrirPareamento);
    println!(
        "monitor na porta {}; pareamento aberto enquanto a bancada roda",
        ir_monitor::PORTA
    );

    let (amostras_tx, mut amostras) = mpsc::unbounded_channel();
    let _vigia = ir_telemetria::Vigia::iniciar(move |amostra| amostras_tx.send(amostra).is_ok())?;
    let mut mesa = Mesa::nova("bancada");
    mesa.par_conectado("notebook (simulado)", true);
    let inicio = Instant::now();
    // A janela fecha em 3 minutos; numa bancada ela fica aberta enquanto a bancada roda.
    let mut reabrir = tokio::time::interval(Duration::from_secs(120));

    loop {
        tokio::select! {
            _ = reabrir.tick() => monitor.comandar(Comando::AbrirPareamento),
            Some(aviso) = avisos.recv() => {
                if let Aviso::Codigo(digitos) = &aviso {
                    println!("código {digitos:?} — confirmando sozinho");
                    monitor.comandar(Comando::Confirmar(true));
                } else {
                    println!("{aviso:?}");
                }
            }
            Some(amostra) = amostras.recv() => {
                let agora = Instant::now();
                let passo = inicio.elapsed().as_secs();
                mesa.anotar_par(agora, par_simulado(&amostra, passo));
                mesa.anotar_local(amostra);
                mesa.anotar_volta(Rota::Bluetooth, agora, oscilar(7_000, passo, 2_500));
                mesa.anotar_volta(Rota::Rede, agora, oscilar(3_000, passo, 1_200));
                monitor.publicar(mesa.quadro(agora));
            }
        }
    }
}

/// O par de mentira: a amostra daqui, com outros números, para os dois cartões não serem iguais.
fn par_simulado(daqui: &UsageSample, passo: u64) -> UsageSample {
    let mut par = daqui.clone();
    par.cpu_permille = Some(u16::try_from(oscilar(180, passo, 120)).unwrap_or(0));
    par.battery = Some(ir_proto::message::Battery {
        percent: 64,
        plugged: false,
    });
    par
}

/// Um número que sobe e desce devagar em torno de `base`.
fn oscilar(base: u32, passo: u64, amplitude: u32) -> u32 {
    let fase = u32::try_from(passo % 20).unwrap_or(0);
    let triangulo = if fase < 10 { fase } else { 20 - fase };
    base + amplitude * triangulo / 10
}
