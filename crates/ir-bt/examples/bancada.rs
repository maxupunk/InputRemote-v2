//! Bancada do rádio: um enlace RFCOMM de verdade entre dois computadores, e a latência dele.
//!
//! Existe porque o hardware é a única parte que o CI não roda. Os testes do crate provam o
//! protocolo com um rádio de mentira; isto prova o rádio, e mede o que só o rádio responde.
//!
//! ```text
//! no que escuta:  bancada escutar
//! no que liga:    bancada conectar AC:50:DE:47:EB:28
//! ```
//!
//! Usa o [`Endpoint`] de produção, e não um caminho paralelo — é o mesmo código do serviço. O
//! que ele faz a mais é confirmar o pareamento sozinho, para o teste não depender de duas
//! pessoas clicando ao mesmo tempo; num uso de verdade quem confirma é o usuário, depois de
//! comparar os seis dígitos nas duas telas.
//!
//! **Quem ecoa é só o lado que escuta.** Os dois ecoando viram um pingue-pongue sem fim, que foi
//! como esta ferramenta nasceu — e o número que ela imprimia não media coisa alguma.
//!
//! Não usa `unwrap`, `expect` nem `panic`: um exemplo não é teste, e as regras de
//! [09, §5](../../../docs/09-padroes-de-codigo.md) valem aqui.

use std::sync::Arc;
use std::time::{Duration, Instant};

use ir_bt::{BdAddr, BtCommand, BtEvent, ConnectMode, Endpoint, EndpointHandle};
use ir_crypto::Identity;

/// Quanto se espera por um evento antes de desistir.
const PRAZO: Duration = Duration::from_secs(60);

/// Quantas idas e voltas medir.
const SONDAS: u64 = 200;

/// O que este lado faz.
enum Papel {
    /// Abre o canal, espera alguém ligar, e devolve cada quadro.
    Escutar,
    /// Liga para o endereço dado e mede a ida e volta.
    Conectar(BdAddr),
}

fn ler_papel() -> Result<Papel, String> {
    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        Some("escutar") => Ok(Papel::Escutar),
        Some("conectar") => match args.next() {
            Some(texto) => match texto.parse::<BdAddr>() {
                Ok(endereco) => Ok(Papel::Conectar(endereco)),
                Err(erro) => Err(format!("endereço inválido: {erro}")),
            },
            None => Err("falta o endereço do par".to_owned()),
        },
        _ => Err("uso: bancada escutar | bancada conectar <AA:BB:CC:DD:EE:FF>".to_owned()),
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let papel = match ler_papel() {
        Ok(papel) => papel,
        Err(erro) => return println!("ERRO: {erro}"),
    };

    let radio = match ir_bt::abrir_radio() {
        Ok(radio) => Arc::new(radio),
        Err(erro) => {
            println!("ERRO: não abriu o rádio: {erro}");
            if let Some(o_que_fazer) = erro.o_que_fazer() {
                println!("      {o_que_fazer}");
            }
            return;
        }
    };
    let identidade = Arc::new(Identity::generate());
    println!("rádio aberto no canal {}", ir_bt::CANAL);
    println!("identidade desta ponta: {}", identidade.fingerprint());

    let mut alca = Endpoint::spawn(radio, identidade);

    match papel {
        Papel::Escutar => {
            println!("esperando o outro computador ligar …");
            refletir(&mut alca).await;
        }
        Papel::Conectar(peer) => {
            println!("ligando para {peer} …");
            let pedido = alca.commands.send(BtCommand::Connect {
                peer,
                mode: ConnectMode::Pair,
            });
            if pedido.is_err() {
                return println!("ERRO: o endpoint não aceitou o comando");
            }
            medir(&mut alca).await;
        }
    }
}

/// Mostra o código e confirma. Devolve `false` se não deu para responder.
fn confirmar(alca: &EndpointHandle, code: [u8; 6], desde: Instant) -> bool {
    let digitos: String = code.iter().map(|d| char::from(b'0' + d)).collect();
    println!(
        "[{:>6} ms] CÓDIGO DE PAREAMENTO: {digitos}",
        desde.elapsed().as_millis()
    );
    println!("            confirmando — compare com a outra tela");
    alca.commands.send(BtCommand::ConfirmPairing(true)).is_ok()
}

/// O lado que escuta: devolve cada quadro como veio, até o enlace cair.
async fn refletir(alca: &mut EndpointHandle) {
    let desde = Instant::now();
    loop {
        let evento = match tokio::time::timeout(PRAZO, alca.events.recv()).await {
            Ok(Some(evento)) => evento,
            Ok(None) => return println!("o endpoint encerrou"),
            Err(_) => return println!("nada aconteceu em {PRAZO:?}; desistindo"),
        };
        match evento {
            BtEvent::PairingCode { code, .. } => {
                if !confirmar(alca, code, desde) {
                    return println!("ERRO: não deu para confirmar");
                }
            }
            BtEvent::Established { peer, .. } => {
                println!(
                    "[{:>6} ms] ENLACE PRONTO com {peer}; refletindo quadros",
                    desde.elapsed().as_millis()
                );
            }
            BtEvent::Frame(bytes) => {
                let _ = alca.commands.send(BtCommand::SendFrame(bytes));
            }
            BtEvent::LinkDown(motivo) => {
                return println!(
                    "[{:>6} ms] ENLACE CAIU: {motivo}",
                    desde.elapsed().as_millis()
                );
            }
            BtEvent::Error(mensagem) => println!("erro: {mensagem}"),
            _ => {}
        }
    }
}

/// O lado que liga: manda uma sonda por vez e cronometra a volta.
async fn medir(alca: &mut EndpointHandle) {
    let desde = Instant::now();
    let mut idas: Vec<Duration> = Vec::new();
    let mut enviada_em = Instant::now();
    let mut proxima: u64 = 0;

    loop {
        let evento = match tokio::time::timeout(PRAZO, alca.events.recv()).await {
            Ok(Some(evento)) => evento,
            Ok(None) => return println!("o endpoint encerrou"),
            Err(_) => return println!("nada aconteceu em {PRAZO:?}; desistindo"),
        };
        match evento {
            BtEvent::PairingCode { code, .. } => {
                if !confirmar(alca, code, desde) {
                    return println!("ERRO: não deu para confirmar");
                }
            }
            BtEvent::Established { peer, .. } => {
                println!(
                    "[{:>6} ms] ENLACE PRONTO com {peer}; medindo {SONDAS} idas e voltas",
                    desde.elapsed().as_millis()
                );
                enviada_em = Instant::now();
                if !sondar(alca, proxima) {
                    return println!("ERRO: não deu para enviar");
                }
                proxima += 1;
            }
            BtEvent::Frame(_) => {
                idas.push(enviada_em.elapsed());
                if proxima >= SONDAS {
                    relatar(&mut idas, desde);
                    let _ = alca.commands.send(BtCommand::Disconnect);
                    return;
                }
                enviada_em = Instant::now();
                if !sondar(alca, proxima) {
                    return println!("ERRO: não deu para enviar");
                }
                proxima += 1;
            }
            BtEvent::LinkDown(motivo) => {
                println!(
                    "[{:>6} ms] ENLACE CAIU: {motivo}",
                    desde.elapsed().as_millis()
                );
                relatar(&mut idas, desde);
                return;
            }
            BtEvent::Error(mensagem) => println!("erro: {mensagem}"),
            _ => {}
        }
    }
}

/// Manda uma sonda numerada. O número volta igual, o que amarra a volta à ida.
fn sondar(alca: &EndpointHandle, numero: u64) -> bool {
    alca.commands
        .send(BtCommand::SendFrame(numero.to_le_bytes().to_vec()))
        .is_ok()
}

/// Imprime o que as medidas dizem, contra os limites da PoC-2.
fn relatar(idas: &mut [Duration], desde: Instant) {
    if idas.is_empty() {
        return println!("nenhuma ida e volta completou");
    }
    idas.sort_unstable();
    let total = idas.len();
    let em = |fracao: usize| -> u128 {
        let indice = (total.saturating_mul(fracao) / 100).min(total.saturating_sub(1));
        idas.get(indice).map_or(0, Duration::as_micros)
    };
    let ms = |micros: u128| format!("{},{:02} ms", micros / 1000, (micros % 1000) / 10);

    println!();
    println!("=== ida e volta sobre RFCOMM, {total} amostras ===");
    println!("  mediana  {}   (limite da PoC-2: 20 ms)", ms(em(50)));
    println!("  p90      {}", ms(em(90)));
    println!("  p99      {}   (limite da PoC-2: 50 ms)", ms(em(99)));
    println!("  pior     {}", ms(em(100)));
    println!(
        "  total    {} ms de relógio desde o começo",
        desde.elapsed().as_millis()
    );
}
