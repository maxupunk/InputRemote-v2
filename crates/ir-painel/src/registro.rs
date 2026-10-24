//! O registro dos avisos da sessão: o que o serviço escreve no log sobre cada um.
//!
//! Saiu do `ir-daemon` quando o painel no Android levou o serviço ao teto de tamanho de crate
//! ([09, §1](../../../docs/09-padroes-de-codigo.md)). É diagnóstico, e não decide nada: lê o aviso e
//! escreve uma frase.

use ir_session::Notice;
use tracing::{debug, info, warn};

/// Registra um aviso da sessão. Nunca inclui conteúdo digitado
/// ([04, §7](../../../docs/04-seguranca.md)).
pub fn registrar(notice: &Notice) {
    match notice {
        Notice::Connected { peer, carrier } => info!(%peer, %carrier, "sessão estabelecida"),
        Notice::Disconnected { reason, will_retry } => {
            info!(?reason, will_retry, "sessão encerrada");
        }
        Notice::ControlMoved { remote } => info!(remote, "controle mudou de lado"),
        Notice::CarrierChanged { carrier, why } => info!(%carrier, ?why, "portador escolhido"),
        Notice::RouteChanged { route, why } => info!(%route, ?why, "rota da sessão mudou"),
        Notice::Reconciled { released, pressed } => {
            debug!(released, pressed, "estado reconciliado");
        }
        Notice::LatencySample(rtt) => debug!(%rtt, "latência medida"),
        Notice::ProtocolError { code, fatal } => warn!(?code, fatal, "erro de protocolo"),
        Notice::EdgeChanged { edge, .. } => debug!(%edge, "borda escolhida aqui"),
        Notice::CopyPasteAdopted(escolha) => {
            info!(
                ligado = escolha.enabled,
                "copiar e colar mudado no par; este acompanhou"
            );
        }
        Notice::ControlReclaimed { here } => info!(here, "controle retomado sem atravessar"),
        Notice::PeerCancelledCopy => info!("o outro computador pediu para parar a cópia"),
        _ => {}
    }
}
