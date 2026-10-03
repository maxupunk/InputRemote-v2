//! O ajudante das pastas compartilhadas ([ADR-0015](../../../docs/adr/0015-pastas-compartilhadas.md)).
//!
//! O trabalho é do `ir-sincronia`; aqui só o que é deste processo: ser o único da sessão, e sair
//! quando o executável em disco for trocado por uma atualização — quem zela por ele (o serviço no
//! Windows, o `systemd` no Linux) sobe o novo.

use tracing::{info, warn};

use crate::clipboard::{atualizacao, instancia};

/// Roda o ajudante das pastas até a sessão acabar ou o executável mudar.
pub(crate) fn servir() {
    let _trava = match instancia::ser_o_unico_das_pastas() {
        Ok(Some(trava)) => trava,
        Ok(None) => {
            info!("já há um ajudante das pastas nesta sessão; este sai");
            return;
        }
        Err(erro) => {
            warn!(%erro, "não consegui conferir se há outro ajudante das pastas; este sai");
            return;
        }
    };
    let executavel = atualizacao::Executavel::este();
    let continuar = || executavel.as_ref().is_none_or(|e| !e.mudou());
    match ir_sincronia::rodar(&continuar) {
        Ok(()) => info!("o ajudante das pastas terminou"),
        Err(erro) => warn!(
            erro = format!("{erro:#}"),
            "o ajudante das pastas não pôde rodar"
        ),
    }
}
