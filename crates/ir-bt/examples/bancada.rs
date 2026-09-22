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
//! pessoas clicando ao mesmo tempo.
//!
//! # Por que a carga importa
//!
//! A primeira versão mandava uma sonda, esperava a volta, e só então mandava a próxima. Isso
//! deixa o enlace **ocioso** entre as sondas, que é o pior caso possível: um rádio sem tráfego
//! entra em modo de economia, e cada ida passa a esperar a próxima janela do enlace. A medida
//! saía alta e a culpa parecia do portador.
//!
//! Agora ele emite a **125 sondas por segundo**, que é a taxa de ponteiro que
//! [01, §6](../../../docs/01-visao-e-escopo.md) pede, **sem esperar resposta** — e casa cada
//! volta com a ida pelo número de sequência que viaja no corpo. Com tráfego contínuo o enlace
//! não adormece, e o que sobra é a latência do meio.
//!
//! # O que este número é, e o que não é
//!
//! É **ida e volta no transporte**. A meta de [01, §6](../../../docs/01-visao-e-escopo.md) é de
//! latência **adicionada**, medida da captura numa máquina à injeção na outra — uma travessia, e
//! incluindo o que o produto acrescenta no caminho. Isto aqui é um *proxy*: serve para comparar
//! portadores e para achar o efeito da carga, não para fechar a linha da tabela.
//!
//! Não usa `unwrap`, `expect` nem `panic`: um exemplo não é teste.

use std::sync::Arc;
use std::time::{Duration, Instant};

use ir_bt::{BdAddr, BtCommand, BtEvent, ConnectMode, Endpoint, EndpointHandle};
use ir_crypto::Identity;

/// Quanto se espera por um evento antes de desistir.
const PRAZO: Duration = Duration::from_secs(60);

/// Quantas sondas medir.
const SONDAS: u64 = 600;

/// Intervalo entre sondas: 8 ms dá 125 por segundo.
const INTERVALO: Duration = Duration::from_millis(8);

/// Quantos bytes carregam o número de sequência.
const SEQUENCIA: usize = 8;

/// O que este lado faz.
enum Papel {
    /// Abre o canal, espera alguém ligar, e devolve cada quadro.
    Escutar,
    /// Liga para o endereço dado e mede a ida e volta sob carga.
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
            if esperar_enlace(&mut alca).await {
                medir_sob_carga(&mut alca).await;
            }
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
    let mut devolvidos: u64 = 0;
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
            BtEvent::Established { peer, .. } => println!(
                "[{:>6} ms] ENLACE PRONTO com {peer}; refletindo quadros",
                desde.elapsed().as_millis()
            ),
            BtEvent::Frame(bytes) => {
                devolvidos += 1;
                let _ = alca.commands.send(BtCommand::quadro(bytes));
            }
            BtEvent::LinkDown(motivo) => {
                return println!(
                    "[{:>6} ms] ENLACE CAIU: {motivo} — {devolvidos} quadros devolvidos",
                    desde.elapsed().as_millis()
                );
            }
            BtEvent::Error(mensagem) => println!("erro: {mensagem}"),
            _ => {}
        }
    }
}

/// Espera o enlace ficar pronto, confirmando o pareamento. Devolve se deu certo.
async fn esperar_enlace(alca: &mut EndpointHandle) -> bool {
    let desde = Instant::now();
    loop {
        let evento = match tokio::time::timeout(PRAZO, alca.events.recv()).await {
            Ok(Some(evento)) => evento,
            Ok(None) => {
                println!("o endpoint encerrou");
                return false;
            }
            Err(_) => {
                println!("nada aconteceu em {PRAZO:?}; desistindo");
                return false;
            }
        };
        match evento {
            BtEvent::PairingCode { code, .. } => {
                if !confirmar(alca, code, desde) {
                    println!("ERRO: não deu para confirmar");
                    return false;
                }
            }
            BtEvent::Established { peer, .. } => {
                println!(
                    "[{:>6} ms] ENLACE PRONTO com {peer}",
                    desde.elapsed().as_millis()
                );
                return true;
            }
            BtEvent::LinkDown(motivo) => {
                println!("ENLACE CAIU antes de medir: {motivo}");
                return false;
            }
            BtEvent::Error(mensagem) => println!("erro: {mensagem}"),
            _ => {}
        }
    }
}

/// Emite sondas a 125 por segundo **sem esperar resposta**, e cronometra cada volta.
async fn medir_sob_carga(alca: &mut EndpointHandle) {
    let total = usize::try_from(SONDAS).unwrap_or(0);
    let mut saida: Vec<Option<Instant>> = vec![None; total];
    let mut idas: Vec<Duration> = Vec::with_capacity(total);
    let mut enviadas: u64 = 0;
    let comeco = Instant::now();

    let mut ticker = tokio::time::interval(INTERVALO);
    let limite = tokio::time::sleep(PRAZO);
    tokio::pin!(limite);

    println!("medindo {SONDAS} sondas a {} por segundo …", 1000 / 8);
    loop {
        tokio::select! {
            _ = ticker.tick(), if enviadas < SONDAS => {
                if let Some(vaga) = saida.get_mut(usize::try_from(enviadas).unwrap_or(0)) {
                    *vaga = Some(Instant::now());
                }
                if !sondar(alca, enviadas) {
                    return println!("ERRO: não deu para enviar");
                }
                enviadas += 1;
            }
            evento = alca.events.recv() => {
                match evento {
                    Some(BtEvent::Frame(bytes)) => {
                        anotar_volta(&bytes, &saida, &mut idas);
                        if idas.len() >= total {
                            let _ = alca.commands.send(BtCommand::Disconnect);
                            return relatar(&mut idas, enviadas, comeco);
                        }
                    }
                    Some(BtEvent::LinkDown(motivo)) => {
                        println!("ENLACE CAIU no meio da medição: {motivo}");
                        return relatar(&mut idas, enviadas, comeco);
                    }
                    Some(BtEvent::Error(mensagem)) => println!("erro: {mensagem}"),
                    Some(_) => {}
                    None => return println!("o endpoint encerrou"),
                }
            }
            () = &mut limite => {
                println!("prazo de {PRAZO:?} esgotado");
                return relatar(&mut idas, enviadas, comeco);
            }
        }
    }
}

/// Casa uma volta com a ida correspondente, pelo número de sequência.
fn anotar_volta(bytes: &[u8], saida: &[Option<Instant>], idas: &mut Vec<Duration>) {
    let Some(numero) = bytes
        .get(..SEQUENCIA)
        .and_then(|fatia| <[u8; SEQUENCIA]>::try_from(fatia).ok())
        .map(u64::from_le_bytes)
    else {
        return; // não é sonda nossa
    };
    if let Some(Some(partiu)) = saida.get(usize::try_from(numero).unwrap_or(usize::MAX)) {
        idas.push(partiu.elapsed());
    }
}

/// Manda uma sonda numerada. O número volta igual, o que amarra a volta à ida.
fn sondar(alca: &EndpointHandle, numero: u64) -> bool {
    alca.commands
        .send(BtCommand::quadro(numero.to_le_bytes().to_vec()))
        .is_ok()
}

/// Imprime o que as medidas dizem, contra os limites de `docs/01` §6.
fn relatar(idas: &mut [Duration], enviadas: u64, comeco: Instant) {
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
    let decorrido = comeco.elapsed();
    let taxa = u128::from(enviadas) * 1000 / decorrido.as_millis().max(1);

    println!();
    println!("=== ida e volta no transporte, sob carga ===");
    println!(
        "  enviadas {enviadas}, respondidas {total}, perdidas {}",
        u64::try_from(total).map_or(0, |t| enviadas.saturating_sub(t))
    );
    println!("  taxa efetiva de emissão: {taxa} sondas/s");
    println!("  mediana  {}", ms(em(50)));
    println!("  p90      {}", ms(em(90)));
    println!("  p99      {}", ms(em(99)));
    println!("  pior     {}", ms(em(100)));
    println!();
    println!(
        "  por travessia (metade da ida e volta): mediana {}, p99 {}",
        ms(em(50) / 2),
        ms(em(99) / 2)
    );
    println!("  a meta de docs/01 §6 é de latência **adicionada** por travessia:");
    println!("  mediana < 20 ms, p99 < 50 ms — e isto aqui é proxy de transporte, não ela.");
}
