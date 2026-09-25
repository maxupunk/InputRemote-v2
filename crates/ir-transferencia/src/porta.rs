//! A porta TCP de arquivos, aberta com insistência.

use std::sync::Arc;

use ir_ipc::transferencia::Motivo;
use ir_transporte::dados::Porta;
use tracing::warn;

use crate::{Ajuste, Entrada, recusar_enquanto};

/// Abre a porta TCP de arquivos, insistindo até conseguir.
///
/// Numa atualização o serviço novo sobe antes de o sistema liberar a porta do antigo. Antes, uma
/// falha aqui recusava toda cópia até o próximo reinício; agora a porta é tentada de novo a cada
/// [`REABRIR_A_PORTA`], e os pedidos que chegam nesse meio são recusados com o motivo. `None` só
/// quando o serviço está saindo.
pub(crate) async fn abrir_a_porta(ajuste: &Ajuste, entrada: &mut Entrada) -> Option<Porta> {
    let mut avisou = false;
    loop {
        match Porta::abrir(ajuste.porta, Arc::clone(&ajuste.identidade)).await {
            Ok(porta) => return Some(porta),
            Err(erro) => {
                if !avisou {
                    warn!(%erro, "não consegui abrir o TCP de arquivos; tentando de novo");
                    avisou = true;
                }
                let motivo = Motivo::Outro(format!("o canal de arquivos não abriu: {erro}"));
                let espera = tokio::time::sleep(REABRIR_A_PORTA);
                recusar_enquanto(ajuste, entrada, espera, &motivo).await?;
            }
        }
    }
}

/// De quanto em quanto tempo se tenta de novo abrir a porta de arquivos.
const REABRIR_A_PORTA: std::time::Duration = std::time::Duration::from_secs(5);
