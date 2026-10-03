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
use ir_transporte::dados::{EnlaceDeDados, FalhaDeEnvio, Remetente};
use tokio::sync::{Mutex, mpsc};
use tracing::{debug, warn};

use crate::Ajuste;
use crate::enviando::enviar;
use crate::recebendo::receber;

/// Conduz um enlace até ele cair.
pub(crate) async fn conduzir(
    enlace: EnlaceDeDados,
    ajuste: &Ajuste,
    entrada: &mut crate::Entrada,
    deposito: &crate::recebendo::Deposito,
) {
    let remetente = Arc::new(Mutex::new(enlace.remetente));
    // A pasta compartilhada usa o mesmo remetente enquanto este enlace viver; o guarda a solta de
    // qualquer jeito que este futuro termine — inclusive largado no meio, quando o par muda.
    deposito.desvio.enlace(Some(Arc::clone(&remetente)));
    let _faixa = SoltarAoSair(deposito.desvio.clone());
    // Sem limite, e de propósito. Era uma fila de 32, e com mais de 32 arquivos ela enchia: a leitura
    // parava esperando vaga, o destino parava esperando a leitura para mandar o `Verified` seguinte,
    // e este lado parava esperando o destino para mandar o bloco seguinte. As respostas são uma por
    // arquivo, e o manifesto já as limita a 10 000 — o teto existe, só não é aqui.
    let (respostas, recebe_respostas) = mpsc::unbounded_channel();

    let _lendo = AbortarAoSair(tokio::spawn(receber(
        enlace.destinatario,
        Arc::clone(&remetente),
        respostas,
        deposito.clone(),
        ajuste.avisos.clone(),
    )));

    // O envio roda aqui, no próprio laço, para poder consumir `pedidos` por referência: a fila de
    // pedidos sobrevive à queda do enlace, e passá-la para uma tarefa a mataria junto.
    enviar(remetente, recebe_respostas, entrada, ajuste).await;
}

/// Aborta a tarefa ao sair de escopo — de qualquer jeito, inclusive quando quem conduz o enlace é
/// largado no meio porque o par mudou.
///
/// Era um `abort()` no fim de [`conduzir`], que não roda quando o futuro é largado: a leitura do
/// enlace velho seguia viva, segurando o socket, ao lado do enlace novo.
struct AbortarAoSair(tokio::task::JoinHandle<()>);

impl Drop for AbortarAoSair {
    fn drop(&mut self) {
        self.0.abort();
    }
}

/// Solta o remetente da faixa da pasta ao sair de escopo.
struct SoltarAoSair(crate::desvio::Desvio);

impl Drop for SoltarAoSair {
    fn drop(&mut self) {
        self.0.enlace(None);
    }
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

/// Uma mensagem da cópia não saiu. Devolve se o enlace segue de pé.
///
/// Se foi o enlace, quem conduz recomeça e a cópia segue a regra da retomada. Se foi a mensagem —
/// não coube num quadro —, o defeito é daqui: a cópia para com o motivo na tela, e o enlace, que
/// nada tem com isso, continua servindo. Um lugar só para essa decisão, porque espalhada ela era
/// "qualquer erro é queda", e foi assim que um manifesto grande demais virou "a conexão de arquivos
/// caiu" ([log 55](../../../docs/logs/55-o-manifesto-que-nao-cabia.md)).
pub(crate) fn falhou_ao_enviar(
    falha: &FalhaDeEnvio,
    avisos: &tokio::sync::broadcast::Sender<Aviso>,
    (nome, progresso): (&str, (u64, u64)),
) -> bool {
    if falha.derrubou_o_enlace() {
        debug!(%falha, "o canal de arquivos caiu no meio de uma cópia");
        return false;
    }
    warn!(%falha, "uma mensagem da cópia não coube num quadro; a cópia para e o enlace segue");
    let motivo = Motivo::Outro(
        "a cópia não pôde ser montada para envio; o registro do serviço tem o detalhe".to_owned(),
    );
    anunciar(
        avisos,
        Sentido::Enviando,
        nome,
        progresso,
        Fase::Parada(motivo),
    );
    true
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
