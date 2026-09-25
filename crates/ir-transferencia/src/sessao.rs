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

/// Conduz um enlace até ele cair.
pub(crate) async fn conduzir(
    enlace: EnlaceDeDados,
    ajuste: &Ajuste,
    entrada: &mut crate::Entrada,
    faxineiro: &Arc<crate::faxina::Faxineiro>,
) {
    let remetente = Arc::new(Mutex::new(enlace.remetente));
    // Sem limite, e de propósito. Era uma fila de 32, e com mais de 32 arquivos ela enchia: a leitura
    // parava esperando vaga, o destino parava esperando a leitura para mandar o `Verified` seguinte,
    // e este lado parava esperando o destino para mandar o bloco seguinte. As respostas são uma por
    // arquivo, e o manifesto já as limita a 10 000 — o teto existe, só não é aqui.
    let (respostas, recebe_respostas) = mpsc::unbounded_channel();

    let lendo = tokio::spawn(receber(
        enlace.destinatario,
        Arc::clone(&remetente),
        respostas,
        crate::recebendo::Deposito {
            pasta: ajuste.recebidos.clone(),
            cota: ajuste.cota,
            faxineiro: Arc::clone(faxineiro),
        },
        ajuste.avisos.clone(),
    ));

    // O envio roda aqui, no próprio laço, para poder consumir `pedidos` por referência: a fila de
    // pedidos sobrevive à queda do enlace, e passá-la para uma tarefa a mataria junto.
    enviar(remetente, recebe_respostas, entrada, ajuste).await;
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

/// Conta à interface que o canal caiu no meio de uma cópia — e só se havia uma.
///
/// `em_curso` é o nome e o andamento da cópia que estava atravessando, ou `None`. Um lugar só para
/// este aviso, porque espalhado ele saía duas vezes numa queda durante o envio (uma de cada camada,
/// a segunda sem nome) e saía também quando o enlace caía em repouso — um cartão de "a cópia não
/// atravessou" sem cópia nenhuma. Devolve se avisou.
pub(crate) fn anunciar_queda(
    avisos: &tokio::sync::broadcast::Sender<Aviso>,
    sentido: Sentido,
    em_curso: Option<(&str, (u64, u64))>,
) -> bool {
    let Some((nome, progresso)) = em_curso else {
        return false;
    };
    let fase = Fase::Parada(Motivo::CanalCaiu);
    anunciar(avisos, sentido, nome, progresso, fase);
    true
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

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn a_queda_sem_copia_em_curso_nao_avisa_nada() {
        // O cartão de "a cópia não atravessou" aparecia quando o enlace caía em repouso.
        let (avisos, mut recebe) = tokio::sync::broadcast::channel(4);
        assert!(!anunciar_queda(&avisos, Sentido::Recebendo, None));
        assert!(recebe.try_recv().is_err());
    }

    #[test]
    fn a_queda_no_meio_de_uma_copia_avisa_uma_vez_com_o_nome_dela() {
        let (avisos, mut recebe) = tokio::sync::broadcast::channel(4);
        let em_curso = Some(("a.txt e outros", (10, 30)));
        assert!(anunciar_queda(&avisos, Sentido::Enviando, em_curso));
        let Ok(Aviso::Transferencia(copia)) = recebe.try_recv() else {
            panic!("esperava o aviso da cópia");
        };
        assert_eq!(copia.nome, "a.txt e outros");
        assert_eq!((copia.bytes_feitos, copia.bytes_total), (10, 30));
        assert_eq!(copia.fase, Fase::Parada(Motivo::CanalCaiu));
        assert!(recebe.try_recv().is_err(), "um aviso só");
    }
}
