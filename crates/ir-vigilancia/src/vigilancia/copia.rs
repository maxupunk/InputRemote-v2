//! A cópia de arquivos no painel: o mesmo aviso que a janela recebe, com a velocidade medida pelo
//! mesmo [`Velocimetro`](ir_ipc::transferencia::Velocimetro) — as duas telas dizem a mesma coisa.

use std::time::Instant;

use ir_ipc::{Aviso, Fase, Sentido, Transferencia};
use ir_monitor::{Copia, FaseDaCopia};
use tokio::sync::broadcast;

use super::{Entregador, Vigilancia};
use crate::Evento;

/// Passa ao laço do serviço cada aviso de cópia, enquanto ele os quiser.
pub(super) fn ouvir(mut avisos: broadcast::Receiver<Aviso>, entregar: Entregador) {
    tokio::spawn(async move {
        loop {
            match avisos.recv().await {
                Ok(Aviso::Transferencia(transferencia)) => {
                    if !entregar(Evento::Copia(transferencia)) {
                        break;
                    }
                }
                // Um aviso perdido na fila cheia é corrigido pelo próximo.
                Ok(_) | Err(broadcast::error::RecvError::Lagged(_)) => {}
                Err(broadcast::error::RecvError::Closed) => break,
            }
        }
    });
}

impl Vigilancia {
    /// Uma cópia andou: na mesa, com a velocidade de agora.
    pub(super) fn copia(&mut self, transferencia: &Transferencia) {
        let (velocimetro, de_qual) = &mut self.velocimetro;
        if de_qual.as_deref() != Some(transferencia.nome.as_str())
            || transferencia.fase == Fase::Anunciada
        {
            velocimetro.zerar();
            *de_qual = Some(transferencia.nome.clone());
        }
        let agora = Instant::now();
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let por_segundo = velocimetro
            .medir(transferencia.bytes_feitos, agora)
            .map(|b| b as u64);
        self.mesa
            .anotar_copia(agora, para_o_painel(transferencia, por_segundo));
    }
}

/// A cópia como o painel a mostra: o nome, nunca o caminho, e o motivo em português.
pub(super) fn para_o_painel(transferencia: &Transferencia, por_segundo: Option<u64>) -> Copia {
    let fase = match &transferencia.fase {
        Fase::Anunciada => FaseDaCopia::Esperando,
        Fase::Concluida { .. } => FaseDaCopia::Concluida,
        Fase::Parada(motivo) => FaseDaCopia::Parada(motivo.descricao(transferencia.sentido)),
        Fase::AguardandoConexao => FaseDaCopia::AguardandoConexao,
        // `Andando`, e a fase que um dia vier depois: em curso é o que o painel sabe dizer.
        _ => FaseDaCopia::Andando,
    };
    Copia {
        enviando: transferencia.sentido == Sentido::Enviando,
        nome: transferencia.nome.clone(),
        feitos: transferencia.bytes_feitos,
        total: transferencia.bytes_total,
        por_segundo,
        fase,
    }
}
