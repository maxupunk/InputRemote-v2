//! Cliente de controle sem interface gráfica: fala com o serviço pelo canal de controle.
//!
//! A janela é a forma normal de operar o produto. Isto existe para a bancada, onde não há quem
//! clique: o pareamento **começa** por um pedido ao serviço
//! ([log 25](../../../docs/logs/25-o-pareamento-que-se-desfazia-depois-do-clique.md)), e sem um
//! cliente não há como começá-lo de um teste automatizado.
//!
//! ```text
//! controle <endereco> estado
//! controle <endereco> diagnostico
//! controle <endereco> parear AC:50:DE:47:EB:28   # pede, espera o código e confirma
//! controle <endereco> aguardar                   # só espera o código e confirma
//! controle <endereco> confirmar                  # confirma agora, se houver código na tela
//! controle <endereco> enviar C:\caminho\arquivo      # manda arquivos e acompanha o progresso
//! ```
//!
//! `aguardar` existe por causa de uma corrida: quem **recebe** o pareamento só tem o que
//! confirmar depois que o outro lado discou. Mandar `confirmar` antes disso recebe
//! `PareamentoInterrompido` e mata o pareamento — então este lado espera o código aparecer.
//!
//! Num uso de verdade quem confirma é a pessoa, depois de comparar os seis dígitos nas duas
//! telas. Aqui a confirmação é automática porque numa bancada não há duas pessoas.
//!
//! **Quem pode falar aqui é o sistema operacional que decide**, pelo descritor do canal — este
//! programa não ganha privilégio nenhum por existir.
//!
//! Não usa `unwrap`, `expect` nem `panic`: um exemplo não é teste.

use std::io::{Read, Write};

use ir_ipc::{ParaInterface, Pedido, Resposta, codec};

/// Quantas mensagens ler antes de desistir, para a bancada não ficar presa.
const TETO_DE_MENSAGENS: usize = 60;

/// O que este lado vai fazer.
struct Roteiro {
    /// O pedido a mandar, se houver algum além de acompanhar.
    pedido: Option<Pedido>,
    /// Se deve confirmar sozinho quando o código aparecer.
    confirmar_sozinho: bool,
}

fn main() {
    let mut args = std::env::args().skip(1);
    let (Some(endereco), Some(acao)) = (args.next(), args.next()) else {
        return println!(
            "uso: controle <endereco> estado|diagnostico|aguardar|confirmar|parear <par>|enviar <caminho>"
        );
    };

    let roteiro = match montar(&acao, args.next()) {
        Ok(roteiro) => roteiro,
        Err(erro) => return println!("ERRO: {erro}"),
    };

    let (mut escritor, mut leitor) = match ir_ipc::cliente::abrir(&endereco) {
        Ok(duplex) => duplex,
        Err(erro) => return println!("ERRO: não abriu {endereco}: {erro}"),
    };

    // Acompanhar primeiro: os avisos que interessam (o código, a mudança de estado) não são
    // resposta a pedido nenhum, e sem isto eles não chegariam.
    if !pedir(&mut escritor, &Pedido::Acompanhar) {
        return;
    }
    if let Some(pedido) = roteiro.pedido.as_ref()
        && !pedir(&mut escritor, pedido)
    {
        return;
    }

    escutar(&mut leitor, &mut escritor, roteiro.confirmar_sozinho);
}

/// Traduz a ação da linha de comando no roteiro correspondente.
fn montar(acao: &str, argumento: Option<String>) -> Result<Roteiro, String> {
    let simples = |pedido| {
        Ok(Roteiro {
            pedido: Some(pedido),
            confirmar_sozinho: false,
        })
    };
    match acao {
        "estado" => simples(Pedido::Estado),
        "diagnostico" => simples(Pedido::Diagnostico),
        "confirmar" => simples(Pedido::ConfirmarPareamento { conferiu: true }),
        "aguardar" => Ok(Roteiro {
            pedido: None,
            confirmar_sozinho: true,
        }),
        // Vários caminhos separados por `;`, para dar conta do caso de copiar mais de uma coisa.
        "enviar" => match argumento {
            Some(lista) => {
                let caminhos: Vec<String> = lista
                    .split(';')
                    .map(str::trim)
                    .filter(|caminho| !caminho.is_empty())
                    .map(str::to_owned)
                    .collect();
                if caminhos.is_empty() {
                    return Err("nenhum caminho para enviar".to_owned());
                }
                Ok(Roteiro {
                    pedido: Some(Pedido::EnviarArquivos { caminhos }),
                    // A transferência acontece depois da resposta, e o que interessa vem por
                    // aviso: ficar escutando é o ponto.
                    confirmar_sozinho: true,
                })
            }
            None => Err("falta o caminho a enviar".to_owned()),
        },
        "parear" => match argumento {
            Some(candidato) => Ok(Roteiro {
                pedido: Some(Pedido::IniciarPareamento { candidato }),
                confirmar_sozinho: true,
            }),
            None => Err("falta o endereço do par".to_owned()),
        },
        outra => Err(format!("ação desconhecida: {outra}")),
    }
}

/// Manda um pedido. Devolve `false` se não deu.
fn pedir(escritor: &mut Box<dyn Write + Send>, pedido: &Pedido) -> bool {
    let bytes = match codec::codificar(pedido) {
        Ok(bytes) => bytes,
        Err(erro) => {
            println!("ERRO: não codificou: {erro}");
            return false;
        }
    };
    if let Err(erro) = escritor.write_all(&bytes) {
        println!("ERRO: não escreveu: {erro}");
        return false;
    }
    if let Err(erro) = escritor.flush() {
        println!("ERRO: não esvaziou: {erro}");
        return false;
    }
    true
}

/// Lê uma mensagem do serviço. `None` quando o canal acaba ou o corpo não decodifica.
fn ler(leitor: &mut Box<dyn Read + Send>) -> Option<ParaInterface> {
    let mut prefixo = [0u8; codec::PREFIXO];
    if leitor.read_exact(&mut prefixo).is_err() {
        return None;
    }
    let tamanho = codec::tamanho_anunciado(&prefixo).ok()?;
    let mut corpo = vec![0u8; tamanho];
    if leitor.read_exact(&mut corpo).is_err() {
        return None;
    }
    codec::decodificar::<ParaInterface>(&corpo).ok()
}

/// Acompanha o que o serviço diz, confirmando o pareamento quando for o caso.
fn escutar(
    leitor: &mut Box<dyn Read + Send>,
    escritor: &mut Box<dyn Write + Send>,
    confirmar_sozinho: bool,
) {
    for _ in 0..TETO_DE_MENSAGENS {
        let Some(mensagem) = ler(leitor) else {
            return println!("o canal encerrou");
        };
        let terminou = match mensagem {
            ParaInterface::Resposta(resposta) => mostrar_resposta(&resposta),
            ParaInterface::Aviso(aviso) => mostrar_aviso(&aviso, escritor, confirmar_sozinho),
            _ => false,
        };
        if terminou {
            return;
        }
    }
    println!("teto de {TETO_DE_MENSAGENS} mensagens; encerrando");
}

/// Mostra uma resposta. Devolve `true` quando não há mais o que esperar.
fn mostrar_resposta(resposta: &Resposta) -> bool {
    match resposta {
        Resposta::Estado(estado) => {
            println!("ESTADO");
            println!("  enlace:   {:?}", estado.enlace);
            println!("  papel:    {:?}", estado.papel);
            println!("  portador: {:?}", estado.portador);
            println!("  motivo:   {:?}", estado.motivo_do_portador);
            println!("  par:      {:?}", estado.par.as_ref().map(|p| p.conectado));
            // Sem agente pronto nada e digitado nesta maquina: no Windows quem captura e injeta
            // e ele, e o servico na sessao 0 nao alcanca a area de trabalho de ninguem.
            println!("  agente:   pronto={}", estado.agente_pronto);
            println!("  nivel:    {:?}", estado.nivel_privilegiado);
            // Consultar o estado é pergunta, não assinatura: quem quer acompanhar usa
            // `aguardar`. Continuar escutando aqui prendia a bancada num `read_exact` à espera
            // de mensagens que só chegam quando algo muda.
            true
        }
        Resposta::Diagnostico(texto) => {
            println!("DIAGNÓSTICO\n{texto}");
            true
        }
        Resposta::Falha(falha) => {
            println!("FALHA: {falha:?} — {}", falha.o_que_fazer());
            true
        }
        // `Feito` não tem o que mostrar, e uma variante nova não pode derrubar a bancada.
        _ => false,
    }
}

/// Mostra um aviso. Devolve `true` quando não há mais o que esperar.
fn mostrar_aviso(
    aviso: &ir_ipc::Aviso,
    escritor: &mut Box<dyn Write + Send>,
    confirmar_sozinho: bool,
) -> bool {
    match aviso {
        ir_ipc::Aviso::Transferencia(t) => {
            let por_cento = (t.progresso() * 100.0).round();
            println!(
                "TRANSFERÊNCIA [{}] {} — {}/{} B ({por_cento:.0}%) — {:?}",
                t.sentido.rotulo(),
                if t.nome.is_empty() { "(sem nome)" } else { &t.nome },
                t.bytes_feitos,
                t.bytes_total,
                t.fase
            );
            if let ir_ipc::Fase::Parada(motivo) = &t.fase {
                println!("  motivo: {}", motivo.descricao());
            }
            // Acabou de vez: a bancada não tem por que continuar pendurada.
            !t.em_curso()
        }
        ir_ipc::Aviso::CodigoDePareamento { digitos } => {
            let texto: String = digitos.iter().map(|d| char::from(b'0' + d)).collect();
            println!("CÓDIGO DE PAREAMENTO: {texto}");
            if confirmar_sozinho {
                println!("  confirmando — compare com a outra tela");
                let _ = pedir(escritor, &Pedido::ConfirmarPareamento { conferiu: true });
            }
            false
        }
        ir_ipc::Aviso::PareamentoConcluido { sucesso } => {
            println!(
                "PAREAMENTO {}",
                if *sucesso {
                    "CONCLUÍDO"
                } else {
                    "SEM SUCESSO"
                }
            );
            *sucesso
        }
        ir_ipc::Aviso::EstadoMudou(estado) => {
            println!(
                "estado mudou: enlace={:?} portador={:?} motivo={:?}",
                estado.enlace, estado.portador, estado.motivo_do_portador
            );
            false
        }
        ir_ipc::Aviso::CandidatosEncontrados { candidatos } => {
            for candidato in candidatos {
                println!(
                    "candidato: {} em {} por {:?}",
                    candidato.rotulo, candidato.endereco, candidato.portador
                );
            }
            false
        }
        outro => {
            println!("aviso: {outro:?}");
            false
        }
    }
}
