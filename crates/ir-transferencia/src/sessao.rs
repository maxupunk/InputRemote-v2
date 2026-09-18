//! Um enlace de dados em uso: recebendo de um lado, enviando do outro, ao mesmo tempo.
//!
//! # Duas tarefas, um remetente
//!
//! Enquanto este lado despeja blocos, o outro devolve `Verified`. As duas coisas acontecem juntas,
//! então são duas tarefas — mas as respostas de quem **recebe** (`Accept`, `Verified`) saem pelo
//! mesmo socket por onde quem **envia** despeja. Daí o `Mutex` no remetente.
//!
//! Ele não custa o que parece: a seção crítica é uma escrita, sem decisão dentro, e as respostas
//! são raras ao lado dos blocos. O que ele evita é um segundo socket só para o sentido de volta.
//!
//! # Quem responde a quê
//!
//! A tarefa que lê é a única que toca o socket de entrada, e ela separa o que chega em dois:
//! o que pertence a uma transferência que **estamos recebendo** vira estado em disco; o que é
//! resposta a uma transferência que **estamos enviando** é repassado por um canal para a outra
//! tarefa. Sem essa separação, as duas metades brigariam pela mesma mensagem.

use std::path::PathBuf;
use std::sync::Arc;

use ir_ipc::Aviso;
use ir_ipc::transferencia::{Fase, Motivo, Sentido, Transferencia};
use ir_proto::message::{BulkMessage, RejectReason};
use ir_transporte::dados::{EnlaceDeDados, Remetente};
use tokio::sync::{Mutex, mpsc};
use tracing::debug;

use crate::Ajuste;
use crate::enviando::enviar;
use crate::recebendo::receber;

/// Quantas respostas de uma transferência nossa cabem na fila antes de o leitor esperar.
///
/// Pequena de propósito: se o lado que envia parou de ler as respostas, encher a fila é o sintoma
/// certo, e não guardar megabytes de confirmações.
const RESPOSTAS_EM_VOO: usize = 32;

/// Conduz um enlace até ele cair.
pub(crate) async fn conduzir(
    enlace: EnlaceDeDados,
    ajuste: &Ajuste,
    pedidos: &mut mpsc::UnboundedReceiver<Vec<PathBuf>>,
) {
    let remetente = Arc::new(Mutex::new(enlace.remetente));
    let (respostas, recebe_respostas) = mpsc::channel(RESPOSTAS_EM_VOO);

    let lendo = tokio::spawn(receber(
        enlace.destinatario,
        Arc::clone(&remetente),
        respostas,
        crate::recebendo::Deposito {
            pasta: ajuste.recebidos.clone(),
            cota: ajuste.cota,
        },
        ajuste.avisos.clone(),
    ));

    // O envio roda aqui, no próprio laço, para poder consumir `pedidos` por referência: a fila de
    // pedidos sobrevive à queda do enlace, e passá-la para uma tarefa a mataria junto.
    enviar(remetente, recebe_respostas, pedidos, ajuste).await;
    lendo.abort();
}

/// Manda uma resposta, engolindo a falha: quem detecta queda é quem lê.
pub(crate) async fn responder(remetente: &Arc<Mutex<Remetente>>, mensagem: BulkMessage) {
    if let Err(erro) = remetente.lock().await.enviar_agora(mensagem).await {
        debug!(%erro, "não consegui responder no canal de arquivos");
    }
}

/// Conta à interface o que está acontecendo.
pub(crate) fn anunciar(
    avisos: &tokio::sync::broadcast::Sender<Aviso>,
    sentido: Sentido,
    nome: &str,
    // Bytes feitos e bytes no total, juntos porque um sem o outro não diz nada — e porque um
    // parâmetro a mais aqui passaria do teto de cinco de `docs/09` §1.
    progresso: (u64, u64),
    fase: Fase,
) {
    let _ = avisos.send(Aviso::Transferencia(Transferencia {
        sentido,
        nome: nome.to_owned(),
        bytes_feitos: progresso.0,
        bytes_total: progresso.1,
        fase,
    }));
}

/// O motivo do protocolo, no vocabulário da interface.
pub(crate) const fn traduzir_recusa(motivo: RejectReason) -> Motivo {
    match motivo {
        RejectReason::OverQuota => Motivo::AcimaDaCota,
        RejectReason::NoDiskSpace => Motivo::SemEspaco,
        RejectReason::UnsafePath => Motivo::CaminhoInseguro,
        RejectReason::TooManyItems => Motivo::ItensDemais,
        _ => Motivo::SemPermissao,
    }
}
