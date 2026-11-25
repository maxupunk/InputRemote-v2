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
    let pedidos = ir_transferencia::iniciar(ir_transferencia::Ajuste {
        porta: cfg.port,
        recebidos: cfg.pasta_de_recebidos(dir),
        cota: ir_transferencia::Cota::default(),
        identidade: Arc::clone(identidade),
        destino: destino(cfg),
        localizar: ir_transferencia::da_descoberta(descoberta),
        avisos: avisos.clone(),
    });
    pedidos.copias().ligar(cfg.copiar_e_colar);
    pedidos
}

/// Sobe o canal do ajudante das pastas compartilhadas, ligado à faixa do canal de arquivos.
///
/// Sem ele o serviço segue: as pastas ficam indisponíveis, e a janela diz que o ajudante não está.
pub(crate) fn abrir_pastas(
    arquivos: &ir_transferencia::Pedidos,
    avisos: &tokio::sync::broadcast::Sender<ir_ipc::Aviso>,
    (cfg, dir): (&config::Config, &std::path::Path),
) -> Option<ir_canais::Pastas> {
    if let Some(par) = cfg.peers.first()
        && let Some(versao) = ir_configuracao::versao_do_par::guardada(dir, &par.pubkey)
    {
        arquivos.informar_par(Some(versao), par.nome.as_deref().unwrap_or_default());
    }
    let faixa = arquivos.tomar_faixa()?;
    match ir_canais::iniciar_pastas(faixa, avisos.clone()) {
        Ok(pastas) => {
            tracing::info!(endereco = %ir_canais::endereco_das_pastas(), "canal das pastas no ar");
            // No Windows o serviço sobe o ajudante na sessão do usuário; no Linux, o `systemd`.
            ir_sessao::zelar_pelas_pastas(pastas.ajudantes());
            Some(pastas)
        }
        Err(erro) => {
            tracing::warn!(erro = format!("{erro:#}"), "o canal das pastas não subiu");
            None
        }
    }
}

/// Com quem trocar arquivos, pelo que a configuração diz agora ([`config::Config::endereco_do_par`]).
pub(crate) fn destino(cfg: &config::Config) -> ir_transferencia::Destino {
    ir_transferencia::Destino::da_configuracao(cfg.first_peer_key(), cfg.endereco_do_par(), None)
}
