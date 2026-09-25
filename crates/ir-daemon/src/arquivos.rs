//! Onde o canal de arquivos nasce, a partir da configuração.
//!
//! Uma função só, e ela mora fora do `main` porque é tradução: pega o que o arquivo de
//! configuração diz e entrega o que o [`ir_transferencia`] pede. Tradução no `main` é como um
//! `main` vira quinhentas linhas.

use std::sync::Arc;

use crate::config;

/// Sobe o canal de arquivos, na tarefa dele.
///
/// Fora do ator de propósito: um bloco de 60 KiB não pode passar pelo compasso de 5 ms da entrada
/// ([ADR-0010](../../../docs/adr/0010-canal-de-dados-em-tcp-proprio.md)).
///
pub(crate) fn abrir(
    cfg: &config::Config,
    dir: &std::path::Path,
    identidade: &Arc<ir_crypto::Identity>,
    avisos: &tokio::sync::broadcast::Sender<ir_ipc::Aviso>,
    descoberta: &ir_transporte::Descoberta,
) -> ir_transferencia::Pedidos {
    ir_transferencia::iniciar(ir_transferencia::Ajuste {
        porta: cfg.port,
        recebidos: cfg.pasta_de_recebidos(dir),
        cota: ir_transferencia::Cota::default(),
        identidade: Arc::clone(identidade),
        destino: destino(cfg),
        localizar: ir_transferencia::da_descoberta(descoberta),
        avisos: avisos.clone(),
    })
}

/// Com quem trocar arquivos, pelo que a configuração diz agora ([`config::Config::endereco_do_par`]).
pub(crate) fn destino(cfg: &config::Config) -> ir_transferencia::Destino {
    ir_transferencia::Destino::da_configuracao(cfg.first_peer_key(), cfg.endereco_do_par(), None)
}
