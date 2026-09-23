//! O sentido de saída: um pedido de envio por vez, do manifesto ao último bloco — e à confirmação.
//!
//! Uma transferência de cada vez, de propósito. Duas ao mesmo tempo disputariam o socket bloco a
//! bloco e as duas ficariam lentas; em fila, a primeira termina no tempo dela e a segunda começa.
//!
//! # Toda resposta é casada pelo identificador
//!
//! O destino responde `Accept` ao manifesto e `Verified` a cada arquivo, e tudo chega pela mesma
//! fila. A primeira versão lia **uma** resposta por transferência e nunca consumia os `Verified`:
//! eles ficavam na fila, e o manifesto seguinte recebia como resposta um `Verified` velho da
//! transferência anterior — e desistia achando que o canal tinha caído. Visto na bancada, na segunda
//! cópia do Windows para o Linux; na sessão anterior cada máquina tinha enviado uma vez só.
//!
//! Agora uma resposta só vale para a transferência cujo identificador ela traz, e a origem só diz
//! "concluído" depois de receber a conferência de **todos** os arquivos. É essa volta que permite à
//! origem afirmar que chegou, em vez de apenas que saiu.

use std::sync::Arc;
use std::time::Duration;

use ir_files::{Envio, manifesto};
use ir_ipc::transferencia::{Fase, Motivo, Sentido};
use ir_proto::message::{BulkMessage, RejectReason, TransferId};
use ir_transporte::dados::Remetente;
use tokio::sync::{Mutex, mpsc};
use tracing::{debug, info, warn};

use crate::Ajuste;
use crate::fila::Trabalho;
use crate::sessao::{anunciar, traduzir_recusa};

/// Quanto esperar por uma resposta do destino antes de considerar o canal perdido.
///
/// A resposta ao manifesto depende de o destino conferir cota e criar as pastas; a conferência de
/// um arquivo, de ele terminar de gravar o último bloco. Nenhuma das duas leva mais que alguns
/// segundos num disco saudável — trinta é folga, e não chute.
const PRAZO_DA_RESPOSTA: Duration = Duration::from_secs(30);

/// A fila de respostas do destino, alimentada pela metade que lê o socket.
type Respostas = mpsc::UnboundedReceiver<BulkMessage>;

/// O sentido de saída: um pedido de envio por vez.
pub(crate) async fn enviar(
    remetente: Arc<Mutex<Remetente>>,
    mut respostas: Respostas,
    entrada: &mut crate::Entrada,
    ajuste: &Ajuste,
) {
    let mut proxima = TransferId(1);
    loop {
        let Some(trabalho) = proximo(entrada, &mut respostas).await else {
            return;
        };
        let id = proxima;
        proxima = TransferId(proxima.0.wrapping_add(1));
        let pedido = (trabalho.caminhos, trabalho.leitor);
        let inteiro =
            uma_transferencia((&remetente, entrada), &mut respostas, pedido, id, ajuste).await;
        // A cópia saiu da vez, tenha terminado, sido cancelada ou caído com o enlace.
        entrada.fila.terminou();
        if !inteiro {
            // Interrompido no meio: dizer, para quem ofereceu poder oferecer de novo. Sem isto o
            // ajudante de clipboard daria a cópia por entregue e não a repetiria.
            let fase = Fase::Parada(Motivo::CanalCaiu);
            anunciar(&ajuste.avisos, Sentido::Enviando, "", (0, 0), fase);
            return; // o laço de fora consegue outro enlace
        }
    }
}

/// A próxima cópia a fazer. `None` quando o serviço está saindo ou o enlace caiu.
///
/// Ocioso, espera-se **duas** coisas. Um pedido novo, é claro; mas também o fim da fila de
/// respostas, que fecha no instante em que a metade que lê o socket sai — e ela só sai quando o
/// enlace morreu. Esperando só o pedido, um enlace morto em repouso passava despercebido: o serviço
/// do outro lado reiniciava e este lado continuava achando que estava ligado, sem nunca discar de
/// novo. Visto na bancada.
async fn proximo(entrada: &mut crate::Entrada, respostas: &mut Respostas) -> Option<Trabalho> {
    loop {
        if let Some(trabalho) = entrada.fila.tomar() {
            return Some(trabalho);
        }
        tokio::select! {
            toque = entrada.esperar() => toque?,
            resposta = respostas.recv() => match resposta {
                None => return None, // o leitor saiu: o enlace caiu
                Some(velha) => debug!(?velha, "resposta sem transferência em curso descartada"),
            },
        }
    }
}

/// Conduz um envio inteiro. Devolve `false` quando o enlace caiu.
async fn uma_transferencia(
    (remetente, entrada): (&Arc<Mutex<Remetente>>, &crate::Entrada),
    respostas: &mut Respostas,
    (caminhos, leitor): crate::PedidoDeEnvio,
    id: TransferId,
    ajuste: &Ajuste,
) -> bool {
    let plano = match manifesto::montar(id, &caminhos, leitor).await {
        Ok(plano) => plano,
        Err(erro) => {
            warn!(erro = %erro.sem_caminho(), "não consegui montar o manifesto");
            debug!(%erro, "detalhe do manifesto");
            anunciar(&ajuste.avisos, Sentido::Enviando, "", (0, 0), parada(&erro));
            return true;
        }
    };
    let (nome, total) = (plano.nome.clone(), plano.total);
    let arquivos = plano.itens.iter().filter(|item| !item.is_dir).count();
    info!(itens = plano.itens.len(), total, "enviando arquivos");
    anunciar(
        &ajuste.avisos,
        Sentido::Enviando,
        &nome,
        (0, total),
        Fase::Anunciada,
    );

    let mut envio = Envio::novo(plano);
    let manifesto = envio.manifesto();
    if remetente
        .lock()
        .await
        .enviar_agora(manifesto)
        .await
        .is_err()
    {
        return false;
    }
    match esperar_aceite(respostas, id).await {
        Aceite::Aceito => {}
        Aceite::Recusado(motivo) => {
            warn!(?motivo, "o par recusou a transferência");
            let fase = Fase::Parada(traduzir_recusa(motivo));
            anunciar(&ajuste.avisos, Sentido::Enviando, &nome, (0, total), fase);
            return true;
        }
        Aceite::Caiu => return false,
    }
    match crate::despejo::despejar((remetente, entrada), &mut envio, (&nome, total), ajuste).await {
        crate::despejo::Despejo::Pronto => {}
        crate::despejo::Despejo::Cancelado => return true,
        crate::despejo::Despejo::Caiu => return false,
    }
    concluir(respostas, &envio, (&nome, total, arquivos), ajuste).await
}

/// Espera o destino conferir cada arquivo, e só então diz ao usuário que chegou.
async fn concluir(
    respostas: &mut Respostas,
    envio: &Envio,
    (nome, total, arquivos): (&str, u64, usize),
    ajuste: &Ajuste,
) -> bool {
    let feitos = (envio.enviados(), total);
    match esperar_conferencia(respostas, envio.id(), arquivos).await {
        Some(true) => {
            info!(
                bytes = envio.enviados(),
                arquivos, "envio concluído e conferido pelo destino"
            );
            // Quem envia não sabe onde o arquivo ficou do outro lado, e não deve saber: o caminho é
            // da outra máquina. Quem recebe é que preenche este campo, para a própria interface.
            let fase = Fase::Concluida {
                destino: String::new(),
            };
            anunciar(&ajuste.avisos, Sentido::Enviando, nome, feitos, fase);
            true
        }
        Some(false) => {
            warn!("o destino conferiu e o resumo não bateu");
            let fase = Fase::Parada(Motivo::ResumoDivergente);
            anunciar(&ajuste.avisos, Sentido::Enviando, nome, feitos, fase);
            true
        }
        None => false,
    }
}

/// A resposta ao manifesto.
#[derive(Debug, PartialEq, Eq)]
enum Aceite {
    Aceito,
    Recusado(RejectReason),
    Caiu,
}

/// Espera a resposta **a este** manifesto, descartando o que for de outra transferência.
async fn esperar_aceite(respostas: &mut Respostas, id: TransferId) -> Aceite {
    loop {
        match tokio::time::timeout(PRAZO_DA_RESPOSTA, respostas.recv()).await {
            Ok(Some(BulkMessage::Accept { id: de })) if de == id => return Aceite::Aceito,
            Ok(Some(BulkMessage::Reject { id: de, reason })) if de == id => {
                return Aceite::Recusado(reason);
            }
            Ok(Some(outra)) => debug!(?outra, "resposta de outra transferência descartada"),
            Ok(None) | Err(_) => return Aceite::Caiu,
        }
    }
}

/// Espera a conferência de `arquivos` arquivos desta transferência.
///
/// `Some(true)` quando todos conferiram, `Some(false)` no primeiro que não bateu, `None` quando o
/// canal caiu ou o destino calou.
async fn esperar_conferencia(
    respostas: &mut Respostas,
    id: TransferId,
    arquivos: usize,
) -> Option<bool> {
    let mut faltam = arquivos;
    while faltam > 0 {
        match tokio::time::timeout(PRAZO_DA_RESPOSTA, respostas.recv()).await {
            Ok(Some(BulkMessage::Verified { id: de, ok, .. })) if de == id => {
                if !ok {
                    return Some(false);
                }
                faltam -= 1;
            }
            Ok(Some(outra)) => debug!(?outra, "resposta de outra transferência descartada"),
            Ok(None) | Err(_) => return None,
        }
    }
    Some(true)
}

/// Uma falha local, no vocabulário da interface.
pub(crate) fn parada(erro: &ir_files::FileError) -> Fase {
    Fase::Parada(Motivo::Outro(erro.to_string()))
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn fila(mensagens: Vec<BulkMessage>) -> Respostas {
        let (entrada, saida) = mpsc::unbounded_channel();
        for mensagem in mensagens {
            entrada.send(mensagem).unwrap();
        }
        saida
    }

    fn conferido(id: u32, item: u32, ok: bool) -> BulkMessage {
        BulkMessage::Verified {
            id: TransferId(id),
            item,
            ok,
        }
    }

    #[tokio::test]
    async fn um_verified_velho_nao_e_tomado_por_aceite() {
        // O defeito da bancada: a transferência 1 deixou `Verified` na fila, e o manifesto da 2
        // recebia um deles como resposta.
        let mut respostas = fila(vec![
            conferido(1, 0, true),
            conferido(1, 1, true),
            BulkMessage::Accept { id: TransferId(2) },
        ]);
        assert_eq!(
            esperar_aceite(&mut respostas, TransferId(2)).await,
            Aceite::Aceito
        );
    }

    #[tokio::test]
    async fn a_recusa_desta_transferencia_e_reconhecida() {
        let mut respostas = fila(vec![BulkMessage::Reject {
            id: TransferId(3),
            reason: RejectReason::OverQuota,
        }]);
        assert_eq!(
            esperar_aceite(&mut respostas, TransferId(3)).await,
            Aceite::Recusado(RejectReason::OverQuota)
        );
    }

    #[tokio::test]
    async fn o_canal_fechado_e_queda_e_nao_espera_eterna() {
        let (entrada, mut respostas) = mpsc::unbounded_channel();
        drop(entrada);
        assert_eq!(
            esperar_aceite(&mut respostas, TransferId(1)).await,
            Aceite::Caiu
        );
        assert_eq!(
            esperar_conferencia(&mut respostas, TransferId(1), 1).await,
            None
        );
    }

    #[tokio::test]
    async fn so_conclui_com_a_conferencia_de_todos_os_arquivos() {
        let mut respostas = fila(vec![
            conferido(5, 0, true),
            conferido(4, 0, true), // de outra transferência
            conferido(5, 1, true),
            conferido(5, 2, true),
        ]);
        assert_eq!(
            esperar_conferencia(&mut respostas, TransferId(5), 3).await,
            Some(true)
        );
    }

    #[tokio::test]
    async fn um_resumo_que_nao_bateu_para_a_espera() {
        let mut respostas = fila(vec![conferido(6, 0, true), conferido(6, 1, false)]);
        assert_eq!(
            esperar_conferencia(&mut respostas, TransferId(6), 3).await,
            Some(false)
        );
    }

    #[tokio::test]
    async fn uma_arvore_so_de_pastas_nao_espera_conferencia_nenhuma() {
        let mut respostas = fila(Vec::new());
        assert_eq!(
            esperar_conferencia(&mut respostas, TransferId(7), 0).await,
            Some(true)
        );
    }
}
