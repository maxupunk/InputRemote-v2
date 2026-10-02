//! A conversa com o serviço: ligar, pedir para acompanhar o clipboard, trazer os avisos, mandar
//! pedidos — e reconhecer quando este processo já não fala a língua do serviço.
//!
//! Separada do laço principal porque é o canal, e não o clipboard: o laço decide o que fazer com
//! cada aviso; aqui só se fala com o outro lado.

use std::io::{Read, Write};
use std::sync::mpsc::Sender;
use std::thread;

use anyhow::{Context, Result};
use ir_ipc::codec;
use ir_ipc::{Falha, ParaInterface, Pedido, Resposta};
use tracing::{debug, warn};

use super::Evento;

/// Liga ao serviço, pede para acompanhar, e põe uma thread lendo o que ele disser.
pub(super) fn conectar(endereco: &str, eventos: &Sender<Evento>) -> Result<Box<dyn Write + Send>> {
    let (mut escrita, leitura) = ir_ipc::cliente::abrir(endereco)
        .with_context(|| format!("abrindo o canal de controle em {endereco}"))?;
    pedir(escrita.as_mut(), &Pedido::AcompanharClipboard)?;
    let eventos = eventos.clone();
    thread::spawn(move || ler_avisos(leitura, &eventos));
    Ok(escrita)
}

/// Traz os avisos do serviço para o laço principal, até a conexão cair.
///
/// O motivo de parar é registrado. Sem ele, "a conexão caiu" cobria três coisas muito diferentes —
/// o serviço fechou, o quadro veio inválido, o canal deu erro — e a cópia que não atravessava não
/// tinha como ser explicada. Custou uma investigação inteira.
fn ler_avisos(mut leitura: Box<dyn Read + Send>, eventos: &Sender<Evento>) {
    loop {
        let mensagem = match crate::ler_quadro::<ParaInterface>(&mut leitura) {
            Ok(Some(mensagem)) => mensagem,
            Ok(None) => {
                debug!("o serviço fechou o canal de controle");
                break;
            }
            Err(erro) if e_incompativel(&erro) => {
                warn!(erro = ?erro, "o serviço mandou um quadro que este ajudante não entende");
                let _ = eventos.send(Evento::Incompativel);
                return;
            }
            Err(erro) => {
                warn!(erro = ?erro, "falha ao ler do canal de controle");
                break;
            }
        };
        match mensagem {
            ParaInterface::Aviso(aviso) => {
                if eventos.send(Evento::Aviso(aviso)).is_err() {
                    return;
                }
            }
            ParaInterface::Resposta(Resposta::Falha(falha)) => {
                if falha == Falha::SemPermissao {
                    warn!(
                        "o serviço recusou o ajudante: o usuário precisa estar no grupo `inputremote`"
                    );
                }
                if eventos.send(Evento::Recusado).is_err() {
                    return;
                }
            }
            // `Feito` e o resto não pedem nada daqui.
            _ => {}
        }
    }
    let _ = eventos.send(Evento::Caiu);
}

/// Se o erro de leitura é de formato, e não de canal: um quadro inteiro que não decodifica.
///
/// Os dois lados são do mesmo pacote, então isto só acontece com um processo que sobreviveu a uma
/// atualização do serviço.
pub(super) fn e_incompativel(erro: &anyhow::Error) -> bool {
    erro.chain().any(|causa| {
        causa
            .downcast_ref::<std::io::Error>()
            .is_some_and(|io| io.kind() == std::io::ErrorKind::InvalidData)
    })
}

/// Manda um pedido ao serviço.
pub(super) fn pedir(escrita: &mut dyn Write, pedido: &Pedido) -> Result<()> {
    codec::escrever_em(escrita, pedido).context("escrevendo o pedido")
}
