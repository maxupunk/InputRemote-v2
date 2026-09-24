//! Quem pode controlar quem e a borda, como ficam no arquivo, e a política com que o serviço sobe.
//!
//! Moram com a configuração porque são a tradução dela: o arquivo guarda texto, a sessão quer
//! [`Policy`] e [`Edge`]. Não há mais papel ([ADR-0014](../../../docs/adr/0014-controle-simetrico.md)):
//! um arquivo antigo, com `role = "server"` ou `"client"`, sobe com os dois controlando um ao outro.

use std::path::Path;

use anyhow::{Result, bail};
use ir_proto::screens::Edge;
use ir_session::Policy;
use tracing::warn;

use crate::Config;

/// A política padrão, no arquivo.
pub(crate) const PADRAO: &str = "ambos";

/// O texto de configuração para uma política.
#[must_use]
pub const fn texto_da_politica(politica: Policy) -> &'static str {
    match politica {
        Policy::Both => PADRAO,
        Policy::OnlyControls => "so-este",
        Policy::OnlyControlled => "so-o-outro",
    }
}

/// A política que o texto nomeia.
///
/// # Errors
///
/// Erro se o texto não for uma das três.
pub fn politica_do_texto(texto: &str) -> Result<Policy> {
    Ok(match texto {
        "ambos" => Policy::Both,
        "so-este" => Policy::OnlyControls,
        "so-o-outro" => Policy::OnlyControlled,
        outro => bail!("política inválida: {outro} (use ambos, so-este ou so-o-outro)"),
    })
}

/// Se esta máquina sustenta a política, sabendo se a plataforma captura a entrada local.
///
/// Só "só este controla" depende de captura sem alternativa: sem ler o teclado daqui, a máquina não
/// faria nada. Com os dois controlando, uma máquina que não captura continua sendo controlada, e a
/// janela diz por que ela não vai para o outro lado.
#[must_use]
pub const fn politica_sustentada(politica: Policy, captura: bool) -> bool {
    captura || !matches!(politica, Policy::OnlyControls)
}

/// O texto de configuração para uma borda.
#[must_use]
pub const fn edge_para_texto(edge: Edge) -> &'static str {
    match edge {
        Edge::Left => "left",
        Edge::Right => "right",
        Edge::Top => "top",
        Edge::Bottom => "bottom",
    }
}

/// A política com que o serviço sobe, corrigindo uma gravada que a plataforma não sustenta.
///
/// Recusar-se a subir seria pior — o `systemd` o reiniciaria em laço. Então ele sobe com os dois
/// controlando, registra em nível alto o que não aplicou, e corrige o arquivo para a janela e a
/// configuração dizerem a mesma coisa.
///
/// # Errors
///
/// Erro se a política gravada não for texto reconhecido. `captura` diz se esta plataforma lê a
/// entrada local.
pub fn politica_na_subida(config: &mut Config, dir: &Path, captura: bool) -> Result<Policy> {
    let gravada = config.policy()?;
    if politica_sustentada(gravada, captura) {
        return Ok(gravada);
    }
    warn!(
        gravada = texto_da_politica(gravada),
        "a política gravada não funciona nesta plataforma, que não captura a entrada local; \
         subindo com os dois controlando e corrigindo a configuração"
    );
    PADRAO.clone_into(&mut config.politica);
    if let Err(erro) = config.save(dir) {
        warn!(%erro, "não foi possível corrigir a política gravada; ela volta na próxima subida");
    }
    Ok(Policy::Both)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_politica_vai_ao_arquivo_e_volta_igual() {
        for politica in [Policy::Both, Policy::OnlyControls, Policy::OnlyControlled] {
            assert_eq!(
                politica_do_texto(texto_da_politica(politica)).unwrap_or_default(),
                politica
            );
        }
        assert!(politica_do_texto("server").is_err(), "papel não é política");
    }

    #[test]
    fn so_este_controla_exige_captura() {
        assert!(!politica_sustentada(Policy::OnlyControls, false));
        assert!(politica_sustentada(Policy::Both, false));
        assert!(politica_sustentada(Policy::OnlyControlled, false));
        assert!(politica_sustentada(Policy::OnlyControls, true));
    }
}
