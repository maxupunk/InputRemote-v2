//! Quem pode controlar quem, a borda e o portador fixado, como ficam no arquivo, e a política com
//! que o serviço sobe.
//!
//! Moram com a configuração porque são a tradução dela: o arquivo guarda texto, a sessão quer
//! [`Policy`], [`Edge`] e [`Carrier`]. Cada texto é uma constante só, usada na ida e na volta: duas
//! grafias do mesmo valor eram como a leitura e a escrita podiam divergir sem ninguém ver. Não há mais papel ([ADR-0014](../../../docs/adr/0014-controle-simetrico.md)):
//! um arquivo antigo, com `role = "server"` ou `"client"`, sobe com os dois controlando um ao outro.

use std::path::Path;

use anyhow::{Result, bail};
use ir_proto::carrier::Carrier;
use ir_proto::screens::Edge;
use ir_session::Policy;
use tracing::warn;

use crate::Config;

/// A política padrão, no arquivo.
pub(crate) const PADRAO: &str = "ambos";
/// Só este controla o outro.
const SO_ESTE: &str = "so-este";
/// Só o outro controla este.
const SO_O_OUTRO: &str = "so-o-outro";

// As bordas, no arquivo.
const ESQUERDA: &str = "left";
const DIREITA: &str = "right";
const ACIMA: &str = "top";
const ABAIXO: &str = "bottom";

// Os portadores que se pode fixar, no arquivo. A rede de arquivos não leva entrada: fixá-la é
// fixar a rede.
const BLUETOOTH: &str = "bluetooth";
const REDE: &str = "rede";

/// O texto de configuração para uma política.
#[must_use]
pub const fn texto_da_politica(politica: Policy) -> &'static str {
    match politica {
        Policy::Both => PADRAO,
        Policy::OnlyControls => SO_ESTE,
        Policy::OnlyControlled => SO_O_OUTRO,
    }
}

/// A política que o texto nomeia.
///
/// # Errors
///
/// Erro se o texto não for uma das três.
pub fn politica_do_texto(texto: &str) -> Result<Policy> {
    Ok(match texto {
        PADRAO => Policy::Both,
        SO_ESTE => Policy::OnlyControls,
        SO_O_OUTRO => Policy::OnlyControlled,
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
        Edge::Left => ESQUERDA,
        Edge::Right => DIREITA,
        Edge::Top => ACIMA,
        Edge::Bottom => ABAIXO,
    }
}

/// A borda que o texto de configuração nomeia.
///
/// # Errors
///
/// Erro se o texto não nomear uma borda.
pub fn edge_do_texto(texto: &str) -> Result<Edge> {
    Ok(match texto {
        ESQUERDA => Edge::Left,
        DIREITA => Edge::Right,
        ACIMA => Edge::Top,
        ABAIXO => Edge::Bottom,
        outro => bail!("borda inválida: {outro}"),
    })
}

/// O portador como fica no arquivo de configuração.
#[must_use]
pub const fn texto_do_portador(portador: Carrier) -> &'static str {
    match portador {
        Carrier::Rfcomm => BLUETOOTH,
        Carrier::Udp | Carrier::Tcp => REDE,
    }
}

/// O portador fixado no arquivo de configuração, se o texto for um dos conhecidos.
#[must_use]
pub fn portador_do_texto(texto: Option<&str>) -> Option<Carrier> {
    match texto? {
        BLUETOOTH => Some(Carrier::Rfcomm),
        REDE => Some(Carrier::Udp),
        _ => None,
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
    fn a_borda_vai_ao_arquivo_e_volta_igual() {
        for edge in [Edge::Left, Edge::Right, Edge::Top, Edge::Bottom] {
            assert_eq!(edge_do_texto(edge_para_texto(edge)).unwrap(), edge);
        }
        assert!(edge_do_texto("meio").is_err());
    }

    #[test]
    fn o_portador_vai_ao_arquivo_e_volta_igual() {
        for portador in [Carrier::Rfcomm, Carrier::Udp] {
            assert_eq!(
                portador_do_texto(Some(texto_do_portador(portador))),
                Some(portador)
            );
        }
        assert_eq!(portador_do_texto(Some("pombo-correio")), None);
        assert_eq!(portador_do_texto(None), None);
    }

    #[test]
    fn so_este_controla_exige_captura() {
        assert!(!politica_sustentada(Policy::OnlyControls, false));
        assert!(politica_sustentada(Policy::Both, false));
        assert!(politica_sustentada(Policy::OnlyControlled, false));
        assert!(politica_sustentada(Policy::OnlyControls, true));
    }
}
