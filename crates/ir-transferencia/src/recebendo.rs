//! O sentido de entrada: o que chega vira arquivo em disco, ou não vira nada.
//!
//! Esta é a metade privilegiada do canal: ela escreve. Toda a verificação mora no `ir-files`, e o
//! que acontece aqui é traduzir o resultado dela em resposta ao par e em aviso para a interface.

use std::path::PathBuf;
use std::sync::Arc;

use ir_files::error::FileError;
use ir_files::{Abertura, Reacao, Recepcao};
use ir_ipc::Aviso;
use ir_ipc::transferencia::{Fase, Motivo, Sentido};
use ir_proto::message::{BulkMessage, TransferId};
use ir_transporte::dados::{Destinatario, Remetente};
use tokio::sync::{Mutex, mpsc};
use tracing::{debug, info, warn};

use crate::sessao::{anunciar, responder, traduzir_recusa};

/// Para onde o que chega vai, e sob que teto.
///
/// Os dois juntos porque são a mesma decisão vista de dois lados — onde gravar e quanto aceitar —,
/// e porque separados passariam do teto de cinco parâmetros de `docs/09` §1.
#[derive(Debug, Clone)]
pub(crate) struct Deposito {
    /// A pasta de recebidos.
    pub(crate) pasta: PathBuf,
    /// Quanto esta máquina aceita.
    pub(crate) cota: ir_files::Cota,
}

/// O sentido de entrada: aplica o que chega e responde.
pub(crate) async fn receber(
    mut destinatario: Destinatario,
    remetente: Arc<Mutex<Remetente>>,
    respostas: mpsc::UnboundedSender<BulkMessage>,
    deposito: Deposito,
    avisos: tokio::sync::broadcast::Sender<Aviso>,
) {
    let mut recepcao: Option<Box<Recepcao>> = None;
    loop {
        let mensagem = match destinatario.receber().await {
            Ok(mensagem) => mensagem,
            Err(erro) => {
                debug!(%erro, "o canal de arquivos encerrou a leitura");
                return;
            }
        };
        // Resposta a algo que **nós** mandamos: é da outra metade.
        if eh_resposta(&mensagem) {
            if respostas.send(mensagem).is_err() {
                debug!("ninguém esperando resposta de transferência");
            }
            continue;
        }
        if let BulkMessage::Manifest {
            id,
            items,
            total_bytes,
        } = mensagem
        {
            recepcao = abrir(&remetente, &avisos, &deposito, (id, items, total_bytes)).await;
            continue;
        }
        let Some(aberta) = recepcao.as_mut() else {
            debug!("mensagem de conteúdo sem transferência aberta");
            continue;
        };
        if aplicar(aberta, mensagem, &remetente, &avisos).await {
            // Terminou, bem ou mal: o estado vai embora e a montagem com ele, se não publicou.
            if let Some(concluida) = recepcao.take() {
                publicar(*concluida, &avisos).await;
            }
        }
    }
}

/// Se esta mensagem é resposta a uma transferência que estamos enviando.
const fn eh_resposta(mensagem: &BulkMessage) -> bool {
    matches!(
        mensagem,
        BulkMessage::Accept { .. } | BulkMessage::Reject { .. } | BulkMessage::Verified { .. }
    )
}

/// Avalia um manifesto e responde.
async fn abrir(
    remetente: &Arc<Mutex<Remetente>>,
    avisos: &tokio::sync::broadcast::Sender<Aviso>,
    deposito: &Deposito,
    manifesto: (TransferId, Vec<ir_proto::message::ManifestItem>, u64),
) -> Option<Box<Recepcao>> {
    let total = manifesto.2;
    let nome = ir_files::publicacao::como_publicar(&manifesto.1);
    let nome = match nome {
        ir_files::Publicacao::Entrada(nome) | ir_files::Publicacao::Agrupadas(nome) => nome,
    };
    match Recepcao::abrir(&deposito.pasta, manifesto, deposito.cota, None).await {
        Ok(Abertura::Aceita { recepcao, resposta }) => {
            info!(total, "recebendo arquivos");
            responder(remetente, resposta).await;
            anunciar(avisos, Sentido::Recebendo, &nome, (0, total), Fase::Andando);
            Some(recepcao)
        }
        Ok(Abertura::Recusada { resposta, motivo }) => {
            warn!(?motivo, "transferência recusada");
            responder(remetente, resposta).await;
            anunciar(
                avisos,
                Sentido::Recebendo,
                &nome,
                (0, total),
                Fase::Parada(traduzir_recusa(motivo)),
            );
            None
        }
        Err(erro) => {
            warn!(%erro, "manifesto impossível");
            None
        }
    }
}

/// Aplica uma mensagem de conteúdo. Devolve `true` quando a transferência terminou.
async fn aplicar(
    recepcao: &mut Recepcao,
    mensagem: BulkMessage,
    remetente: &Arc<Mutex<Remetente>>,
    avisos: &tokio::sync::broadcast::Sender<Aviso>,
) -> bool {
    match recepcao.aplicar(mensagem).await {
        Ok(Reacao::Nada) => false,
        Ok(Reacao::Responder(resposta)) => {
            responder(remetente, resposta).await;
            false
        }
        Ok(Reacao::Concluida(resposta)) => {
            responder(remetente, resposta).await;
            true
        }
        Ok(Reacao::Cancelada(motivo)) => {
            warn!(?motivo, "o par cancelou a transferência");
            anunciar(
                avisos,
                Sentido::Recebendo,
                "",
                (recepcao.escritos(), recepcao.total()),
                Fase::Parada(Motivo::Cancelada),
            );
            true
        }
        Err(erro) => {
            let motivo = if let FileError::ResumoDivergente { item } = erro {
                // Dizer à origem, e não só registrar: ela está esperando a conferência de cada
                // arquivo, e sem resposta esperaria até o prazo para descobrir o que aqui já se sabe.
                let divergiu = BulkMessage::Verified {
                    id: recepcao.id(),
                    item,
                    ok: false,
                };
                responder(remetente, divergiu).await;
                Motivo::ResumoDivergente
            } else {
                Motivo::Outro(erro.to_string())
            };
            warn!(%erro, "transferência interrompida");
            anunciar(
                avisos,
                Sentido::Recebendo,
                "",
                (recepcao.escritos(), recepcao.total()),
                Fase::Parada(motivo),
            );
            true
        }
    }
}

/// Publica a árvore recebida, se ela estiver inteira.
async fn publicar(recepcao: Recepcao, avisos: &tokio::sync::broadcast::Sender<Aviso>) {
    let (feitos, total) = (recepcao.escritos(), recepcao.total());
    match recepcao.concluir().await {
        Ok(destino) => {
            // O caminho completo vai para a interface porque é ele que permite abrir a pasta; para
            // o registro fica em `debug`, que é o teto de `docs/04` §7 para nome de arquivo.
            debug!(?destino, "arquivos recebidos");
            info!(bytes = feitos, "transferência concluída");
            let nome = destino
                .file_name()
                .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
            anunciar(
                avisos,
                Sentido::Recebendo,
                &nome,
                (feitos, total),
                Fase::Concluida {
                    destino: destino.to_string_lossy().into_owned(),
                },
            );
        }
        Err(erro) => debug!(%erro, "a transferência não chegou a ser publicada"),
    }
}
