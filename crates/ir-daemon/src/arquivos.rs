//! Onde o canal de arquivos nasce, a partir da configuração.
//!
//! Uma função só, e ela mora fora do `main` porque é tradução: pega o que o arquivo de
//! configuração diz e entrega o que o [`ir_transferencia`] pede. Tradução no `main` é como um
//! `main` vira quinhentas linhas.

use std::sync::Arc;

use ir_transporte::Endereco;

use crate::config;

/// Sobe o canal de arquivos, na tarefa dele.
///
/// Fora do ator de propósito: um bloco de 60 KiB não pode passar pelo compasso de 5 ms da entrada
/// ([ADR-0010](../../../docs/adr/0010-canal-de-dados-em-tcp-proprio.md)).
///
/// O endereço do par só serve aqui quando é de rede: arquivos nunca viajam pelo rádio
/// ([01, §5](../../../docs/01-visao-e-escopo.md)), e um par alcançável só por Bluetooth tem o canal
/// de arquivos indisponível — declaradamente.
pub(crate) fn abrir(
    cfg: &config::Config,
    dir: &std::path::Path,
    identidade: &Arc<ir_crypto::Identity>,
    avisos: &tokio::sync::broadcast::Sender<ir_ipc::Aviso>,
) -> ir_transferencia::Pedidos {
    let alvo =
        cfg.peer_addr
            .as_deref()
            .and_then(Endereco::ler)
            .and_then(|endereco| match endereco {
                Endereco::Rede(alvo) => Some(alvo),
                Endereco::Radio(_) => None,
            });
    ir_transferencia::iniciar(ir_transferencia::Ajuste {
        porta: cfg.port,
        recebidos: cfg.pasta_de_recebidos(dir),
        cota: ir_transferencia::Cota::default(),
        identidade: Arc::clone(identidade),
        par: cfg.first_peer_key(),
        alvo,
        avisos: avisos.clone(),
    })
}
