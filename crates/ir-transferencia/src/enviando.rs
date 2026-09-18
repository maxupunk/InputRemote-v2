//! O sentido de saída: um pedido de envio por vez, do manifesto ao último bloco.
//!
//! Uma transferência de cada vez, de propósito. Duas ao mesmo tempo disputariam o socket bloco a
//! bloco e as duas ficariam lentas; em fila, a primeira termina no tempo dela e a segunda começa.

use std::path::PathBuf;
use std::sync::Arc;

use ir_files::{Envio, manifesto};
use ir_ipc::transferencia::{Fase, Motivo, Sentido};
use ir_proto::message::{BulkMessage, CancelReason, TransferId};
use ir_transporte::dados::Remetente;
use tokio::sync::{Mutex, mpsc};
use tracing::{info, warn};

use crate::Ajuste;
use crate::sessao::{anunciar, traduzir_recusa};

/// O sentido de saída: um pedido de envio por vez, do manifesto ao último bloco.
pub(crate) async fn enviar(
    remetente: Arc<Mutex<Remetente>>,
    mut respostas: mpsc::Receiver<BulkMessage>,
    pedidos: &mut mpsc::UnboundedReceiver<Vec<PathBuf>>,
    ajuste: &Ajuste,
) {
    let mut proxima = TransferId(1);
    while let Some(caminhos) = pedidos.recv().await {
        let id = proxima;
        proxima = TransferId(proxima.0.wrapping_add(1));
        if !uma_transferencia(&remetente, &mut respostas, caminhos, id, ajuste).await {
            return; // o enlace caiu; o laço de fora consegue outro
        }
    }
}

/// Conduz um envio inteiro. Devolve `false` quando o enlace caiu.
async fn uma_transferencia(
    remetente: &Arc<Mutex<Remetente>>,
    respostas: &mut mpsc::Receiver<BulkMessage>,
    caminhos: Vec<PathBuf>,
    id: TransferId,
    ajuste: &Ajuste,
) -> bool {
    let plano = match manifesto::montar(id, &caminhos).await {
        Ok(plano) => plano,
        Err(erro) => {
            warn!(%erro, "não consegui montar o manifesto");
            anunciar(
                &ajuste.avisos,
                Sentido::Enviando,
                "",
                (0, 0),
                Fase::Parada(Motivo::Outro(erro.to_string())),
            );
            return true;
        }
    };
    let (nome, total) = (plano.nome.clone(), plano.total);
    info!(itens = plano.itens.len(), total, "enviando arquivos");
    anunciar(
        &ajuste.avisos,
        Sentido::Enviando,
        &nome,
        (0, total),
        Fase::Anunciada,
    );

    let mut envio = Envio::novo(plano);
    if remetente
        .lock()
        .await
        .enviar_agora(envio.manifesto())
        .await
        .is_err()
    {
        return false;
    }
    match respostas.recv().await {
        Some(BulkMessage::Accept { .. }) => {}
        Some(BulkMessage::Reject { reason, .. }) => {
            warn!(?reason, "o par recusou a transferência");
            anunciar(
                &ajuste.avisos,
                Sentido::Enviando,
                &nome,
                (0, total),
                Fase::Parada(traduzir_recusa(reason)),
            );
            return true;
        }
        _ => return false,
    }
    despejar(remetente, &mut envio, &nome, total, ajuste).await
}

/// Manda os blocos até acabar. Devolve `false` quando o enlace caiu.
async fn despejar(
    remetente: &Arc<Mutex<Remetente>>,
    envio: &mut Envio,
    nome: &str,
    total: u64,
    ajuste: &Ajuste,
) -> bool {
    loop {
        let proxima = match envio.proxima().await {
            Ok(Some(mensagem)) => mensagem,
            Ok(None) => break,
            Err(erro) => {
                warn!(%erro, "leitura falhou no meio do envio");
                let _ = remetente
                    .lock()
                    .await
                    .enviar_agora(BulkMessage::Cancel {
                        id: TransferId(0),
                        reason: CancelReason::WriteFailed,
                    })
                    .await;
                anunciar(
                    &ajuste.avisos,
                    Sentido::Enviando,
                    nome,
                    (envio.enviados(), total),
                    Fase::Parada(Motivo::Outro(erro.to_string())),
                );
                return true;
            }
        };
        let fim_de_arquivo = matches!(proxima, BulkMessage::FileEnd { .. });
        let mandou = if fim_de_arquivo {
            remetente.lock().await.enviar_agora(proxima).await
        } else {
            remetente.lock().await.enviar(proxima).await
        };
        if mandou.is_err() {
            return false;
        }
        if fim_de_arquivo {
            anunciar(
                &ajuste.avisos,
                Sentido::Enviando,
                nome,
                (envio.enviados(), total),
                Fase::Andando,
            );
        }
    }
    info!(bytes = envio.enviados(), "envio concluído");
    anunciar(
        &ajuste.avisos,
        Sentido::Enviando,
        nome,
        (envio.enviados(), total),
        // Quem envia não sabe onde o arquivo ficou do outro lado, e não deve saber: o caminho é da
        // outra máquina. Quem recebe é que preenche este campo, para a própria interface.
        Fase::Concluida {
            destino: String::new(),
        },
    );
    true
}
