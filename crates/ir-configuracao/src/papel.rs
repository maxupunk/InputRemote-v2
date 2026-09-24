//! O papel e a borda como ficam no arquivo, e o papel com que o serviço sobe.
//!
//! Moram com a configuração porque são a tradução dela: o arquivo guarda texto, a sessão quer
//! [`Role`] e [`Edge`].

use std::path::Path;

use anyhow::Result;
use ir_proto::screens::Edge;
use ir_session::Role;
use tracing::warn;

use crate::Config;

/// Se esta máquina pode assumir `papel`, sabendo se a plataforma captura a entrada local.
///
/// Só o servidor depende de captura: é ele quem tem o teclado. Ser controlado funciona em toda
/// plataforma que injeta.
#[must_use]
pub const fn papel_sustentado(papel: Role, captura: bool) -> bool {
    match papel {
        Role::Server => captura,
        Role::Client => true,
    }
}

/// O texto de configuração para um papel.
#[must_use]
pub const fn texto_do_papel(papel: Role) -> &'static str {
    match papel {
        Role::Server => "server",
        Role::Client => "client",
    }
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

/// O papel com que o serviço sobe, corrigindo um gravado que a plataforma não sustenta.
///
/// Sem isto, um servidor gravado num Linux subia num papel em que nada funciona, e nada dizia por
/// quê. Recusar-se a subir seria pior — o `systemd` o reiniciaria em laço. Então ele sobe como
/// cliente, registra em nível alto o que não aplicou, e corrige o arquivo para a janela e a
/// configuração dizerem a mesma coisa.
///
/// # Errors
///
/// Erro se o papel gravado não for texto reconhecido. `captura` diz se esta plataforma lê a
/// entrada local.
pub fn papel_na_subida(config: &mut Config, dir: &Path, captura: bool) -> Result<Role> {
    let gravado = config.session_role()?;
    if papel_sustentado(gravado, captura) {
        return Ok(gravado);
    }
    warn!(
        gravado = texto_do_papel(gravado),
        "o papel gravado não funciona nesta plataforma, que não captura a entrada local; subindo \
         como cliente e corrigindo a configuração"
    );
    texto_do_papel(Role::Client).clone_into(&mut config.role);
    if let Err(erro) = config.save(dir) {
        warn!(%erro, "não foi possível corrigir o papel gravado; ele volta na próxima subida");
    }
    Ok(Role::Client)
}
