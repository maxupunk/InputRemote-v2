//! Uma bancada para desenvolver o aplicativo Android sem o serviço instalado.
//!
//! ```text
//! cargo run -p ir-monitor-rede --example bancada
//! ```
//!
//! Sobe o monitor de verdade na porta do painel, com a janela de pareamento aberta, a amostra
//! desta máquina pela coleta de verdade e um par de mentira ao lado — para a tela mostrar os dois
//! cartões e a faixa do enlace. Os processos são os desta máquina, medidos como o serviço mede; os
//! do par são os mesmos, com outros números. Uma cópia de mentira passa a cada minuto, para o card
//! da cópia aparecer, andar e sumir. O código de pareamento aparece aqui e é confirmado sozinho: é
//! uma bancada, e quem está nela olha as duas telas.
//!
//! No emulador do Android, o computador é o `10.0.2.2`: digite-o em "IP do computador".

use std::time::Instant;

use ir_crypto::Identity;
use ir_monitor::amostra::UsageSample;
use ir_monitor::{Copia, FaseDaCopia, InteresseEmProcessos, Mesa, Processos, Rota};
use ir_monitor_rede::{Aviso, Comando, Monitor};
use tokio::sync::mpsc;

/// A cópia de mentira: 600 MB a cerca de 40 MB/s, a cada minuto.
const COPIA_TOTAL: u64 = 600 * 1024 * 1024;
const COPIA_POR_SEGUNDO: u64 = 40 * 1024 * 1024;
const COPIA_A_CADA: u64 = 60;

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let (avisos_tx, mut avisos) = mpsc::unbounded_channel();
    let monitor = Monitor::iniciar(Identity::generate(), "bancada", Vec::new(), avisos_tx);
    // Como o serviço de verdade: o aparelho pede, e o código aparece — aqui, confirmado sozinho.
    monitor.comandar(Comando::AceitarPedidos(true));
    println!(
        "monitor na porta {}; aparelhos novos podem pedir para parear",
        ir_monitor::PORTA
    );

    let (amostras_tx, mut amostras) = mpsc::unbounded_channel();
    let vigia = ir_telemetria::Vigia::iniciar(move |amostra, processos| {
        amostras_tx.send((amostra, processos)).is_ok()
    })?;
    let mut interesse: [InteresseEmProcessos; 2] = Default::default();
    let mut mesa = Mesa::nova("bancada");
    mesa.par_conectado("notebook (simulado)", true);
    let inicio = Instant::now();

    loop {
        tokio::select! {
            Some(aviso) = avisos.recv() => match aviso {
                Aviso::Codigo(digitos) => {
                    println!("código {digitos:?} — confirmando sozinho");
                    monitor.comandar(Comando::Confirmar(true));
                }
                Aviso::Interesse(novo) => {
                    println!("interesse: {novo:?}");
                    // O par de mentira mede aqui mesmo: a vigia mede o que os dois querem.
                    vigia.interessar(novo[0].juntar(&novo[1]));
                    interesse = novo;
                }
                outro => println!("{outro:?}"),
            },
            Some((amostra, processos)) = amostras.recv() => {
                let agora = Instant::now();
                let passo = inicio.elapsed().as_secs();
                mesa.anotar_par(agora, par_simulado(&amostra, passo));
                mesa.anotar_local(amostra);
                let [daqui, do_par] = &interesse;
                mesa.anotar_processos(processos.as_ref().and_then(|p| p.para(daqui)));
                if let Some(deles) = processos.as_ref().and_then(|p| p.para(do_par)) {
                    mesa.anotar_processos_do_par(agora, processos_do_par(deles));
                }
                if let Some(copia) = copia_simulada(passo) {
                    mesa.anotar_copia(agora, copia);
                }
                mesa.anotar_volta(Rota::Bluetooth, agora, oscilar(7_000, passo, 2_500));
                mesa.anotar_volta(Rota::Rede, agora, oscilar(3_000, passo, 1_200));
                monitor.publicar(mesa.quadro(agora), mesa.detalhes(agora));
            }
        }
    }
}

/// O par de mentira: a amostra daqui, com outros números, para os dois cartões não serem iguais.
fn par_simulado(daqui: &UsageSample, passo: u64) -> UsageSample {
    let mut par = daqui.clone();
    par.cpu_permille = Some(u16::try_from(oscilar(180, passo, 120)).unwrap_or(0));
    par.battery = Some(ir_monitor::amostra::Battery {
        percent: 64,
        plugged: false,
    });
    par
}

/// Os processos do par de mentira: os daqui, com metade do processador e da memória.
fn processos_do_par(mut processos: Processos) -> Processos {
    for uso in processos.vigiados.iter_mut().chain(&mut processos.lista) {
        uso.cpu_permille /= 2;
        uso.memoria_kib /= 2;
    }
    processos
}

/// A cópia de mentira deste segundo: anda nos primeiros segundos de cada minuto, chega, e depois
/// some sozinha da tela (a mesa a guarda só um pouco depois do fim).
fn copia_simulada(passo: u64) -> Option<Copia> {
    let segundo = passo % COPIA_A_CADA;
    let duracao = COPIA_TOTAL / COPIA_POR_SEGUNDO;
    if segundo > duracao + 1 {
        return None;
    }
    let feitos = (segundo * COPIA_POR_SEGUNDO).min(COPIA_TOTAL);
    let fase = if feitos >= COPIA_TOTAL {
        FaseDaCopia::Concluida
    } else {
        FaseDaCopia::Andando
    };
    Some(Copia {
        enviando: (passo / COPIA_A_CADA).is_multiple_of(2),
        nome: "fotos-das-ferias".to_owned(),
        feitos,
        total: COPIA_TOTAL,
        por_segundo: (segundo > 0).then_some(COPIA_POR_SEGUNDO),
        fase,
    })
}

/// Um número que sobe e desce devagar em torno de `base`.
fn oscilar(base: u32, passo: u64, amplitude: u32) -> u32 {
    let fase = u32::try_from(passo % 20).unwrap_or(0);
    let triangulo = if fase < 10 { fase } else { 20 - fase };
    base + amplitude * triangulo / 10
}
