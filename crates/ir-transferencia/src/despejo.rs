//! O despejo dos blocos de uma cópia.
//!
//! Separado de quem monta o manifesto e espera a conferência porque é outra responsabilidade: aqui
//! não há negociação nenhuma, só bytes saindo — e duas perguntas que se faz a cada bloco. "Já é
//! hora de contar o andamento?", que é o que tirou a tela do 0% eterno; e "o usuário copiou outra
//! coisa?", que é o que faz esta cópia dar lugar à nova em vez de as duas acontecerem.

use std::sync::Arc;

use ir_files::Envio;
use ir_ipc::transferencia::{Fase, Motivo, Sentido};
use ir_proto::message::{BulkMessage, CancelReason};
use ir_transporte::dados::Remetente;
use tokio::sync::Mutex;
use tracing::{debug, info, warn};

use crate::Ajuste;
use crate::enviando::parada;
use crate::sessao::anunciar;

/// Como um despejo de blocos terminou.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Despejo {
    /// Todos os blocos saíram.
    Pronto,
    /// O usuário copiou outra coisa, e esta cópia deu lugar a ela.
    Cancelado,
    /// O enlace caiu.
    Caiu,
}

/// Manda os blocos até acabar, ou até outra cópia tomar a vez.
pub(crate) async fn despejar(
    (remetente, entrada): (&Arc<Mutex<Remetente>>, &crate::Entrada),
    envio: &mut Envio,
    (nome, total): (&str, u64),
    ajuste: &Ajuste,
) -> Despejo {
    // O andamento é contado pelo relógio, e não por arquivo: uma pasta com um arquivo de 2 GB
    // ficava em 0% do começo ao fim ([log 41](../../../docs/logs/41-a-copia-que-se-repetia.md)).
    let mut passo = crate::passo::Passo::novo();
    loop {
        // O usuário copiou outra coisa: esta cópia para aqui, e o outro lado apaga o que já
        // gravou — a montagem dele só vira arquivo de verdade no fim.
        if entrada.fila.cancelando() {
            cancelar(remetente, envio, CancelReason::UserRequested).await;
            info!("cópia cancelada: o usuário copiou outra coisa");
            let feitos = (envio.enviados(), total);
            let fase = Fase::Parada(Motivo::Cancelada);
            anunciar(&ajuste.avisos, Sentido::Enviando, nome, feitos, fase);
            return Despejo::Cancelado;
        }
        let proxima = match envio.proxima().await {
            Ok(Some(mensagem)) => mensagem,
            Ok(None) => return Despejo::Pronto,
            Err(erro) => {
                warn!(erro = %erro.sem_caminho(), "leitura falhou no meio do envio");
                debug!(%erro, "detalhe da leitura que falhou");
                cancelar(remetente, envio, CancelReason::WriteFailed).await;
                let feitos = (envio.enviados(), total);
                anunciar(
                    &ajuste.avisos,
                    Sentido::Enviando,
                    nome,
                    feitos,
                    parada(&erro),
                );
                return Despejo::Pronto;
            }
        };
        let fim_de_arquivo = matches!(proxima, BulkMessage::FileEnd { .. });
        let mandou = if fim_de_arquivo {
            remetente.lock().await.enviar_agora(proxima).await
        } else {
            remetente.lock().await.enviar(proxima).await
        };
        if mandou.is_err() {
            return Despejo::Caiu;
        }
        // O fim de cada arquivo sempre conta; no meio de um arquivo grande, o relógio decide.
        if fim_de_arquivo || passo.passou() {
            let feitos = (envio.enviados(), total);
            anunciar(
                &ajuste.avisos,
                Sentido::Enviando,
                nome,
                feitos,
                Fase::Andando,
            );
        }
    }
}

/// Diz ao outro lado que esta cópia parou, para ele apagar o que já gravou.
///
/// Com o identificador **desta** transferência. Era `TransferId(0)`, e quem recebe ignora mensagem
/// de outra transferência — então o cancelamento nunca chegava, e a recepção do outro lado ficava
/// aberta até o enlace cair. Falhar ao mandar não importa: se o enlace caiu, a montagem de lá vai
/// embora do mesmo jeito.
async fn cancelar(remetente: &Arc<Mutex<Remetente>>, envio: &Envio, motivo: CancelReason) {
    let mensagem = BulkMessage::Cancel {
        id: envio.id(),
        reason: motivo,
    };
    let _ = remetente.lock().await.enviar_agora(mensagem).await;
}
