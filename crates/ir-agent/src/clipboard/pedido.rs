//! O pedido ao serviço que leva cada conteúdo copiado.
//!
//! Arquivos vão pelo canal de dados; texto, pela sessão; imagem, como um arquivo PNG que o outro
//! lado reconhece pelo nome. E o que está numa pasta de rede é trazido para perto antes, porque o
//! serviço não a alcança.

use std::path::PathBuf;

use ir_clip::Conteudo;
use ir_ipc::{Pedido, TextoDoClipboard};
use tracing::warn;

use super::{imagem, rede};

/// Troca o que está numa pasta de rede por uma cópia local, que o serviço consegue ler.
pub(super) fn trazer_da_rede(pedido: Pedido) -> Pedido {
    match pedido {
        Pedido::EnviarArquivos { caminhos } => {
            let originais: Vec<PathBuf> = caminhos.iter().map(PathBuf::from).collect();
            let perto = rede::trazer_para_perto(&originais, &imagem::pasta_temporaria());
            Pedido::EnviarArquivos {
                caminhos: perto
                    .iter()
                    .map(|caminho| caminho.to_string_lossy().into_owned())
                    .collect(),
            }
        }
        outro => outro,
    }
}

/// O pedido que leva este conteúdo, se ele tem caminho.
pub(super) fn pedido_para(conteudo: &Conteudo) -> Option<Pedido> {
    match conteudo {
        Conteudo::Arquivos(caminhos) => Some(Pedido::EnviarArquivos {
            caminhos: caminhos
                .iter()
                .map(|caminho| caminho.to_string_lossy().into_owned())
                .collect(),
        }),
        Conteudo::Texto(texto) => TextoDoClipboard::new(texto.clone()).map(Pedido::OferecerTexto),
        // A imagem vai como arquivo PNG, pelo canal de dados; o outro lado a reconhece pelo nome.
        Conteudo::Imagem(png) => {
            match imagem::gravar_para_enviar(png, &imagem::pasta_temporaria()) {
                Ok(caminho) => Some(Pedido::EnviarArquivos {
                    caminhos: vec![caminho.to_string_lossy().into_owned()],
                }),
                Err(erro) => {
                    warn!(%erro, "não consegui guardar a imagem copiada para enviar");
                    None
                }
            }
        }
        _ => None,
    }
}
