//! Ctrl+C dentro de uma pasta compartilhada.
//!
//! O que está na pasta já existe nos dois computadores. Mandá-lo pela cópia de arquivos faria o
//! conteúdo atravessar de novo — e, se a pessoa colasse na pasta do outro lado, uma terceira vez,
//! pela sincronia. Então a cópia de dentro da pasta não leva bytes: leva os caminhos, pela própria
//! pasta, e o outro computador põe no clipboard dele os mesmos arquivos da cópia dele. Colar lá cola
//! o que a pasta tem; o que ainda não veio vem pela pasta, uma vez só.

use ir_proto::limits::{MAX_COPIED_PATHS, MAX_TCP_PLAINTEXT};
use ir_proto::message::data::is_safe_relative_path;
use ir_proto::message::{FolderId, FolderMessage};
use tracing::{debug, info};

use super::Pastas;
use crate::Saida;

/// Folga para o envelope do quadro em volta dos caminhos.
const ENVELOPE: usize = 1_024;

impl Pastas {
    /// A pessoa copiou estes arquivos da pasta: os caminhos vão ao outro computador.
    pub(super) fn copiar_para_la(
        &self,
        pasta: FolderId,
        caminhos: Vec<String>,
        saida: &mut dyn Saida,
    ) {
        if !self.ambiente().conversando || !self.vivas.contains_key(&pasta) {
            return;
        }
        let paths: Vec<String> = caminhos
            .into_iter()
            .filter(|caminho| is_safe_relative_path(caminho))
            .collect();
        let bytes: usize = paths.iter().map(|c| c.len() + 2).sum();
        if paths.is_empty()
            || paths.len() > MAX_COPIED_PATHS
            || bytes > MAX_TCP_PLAINTEXT - ENVELOPE
        {
            debug!(
                quantos = paths.len(),
                "cópia de dentro da pasta grande demais para levar"
            );
            return;
        }
        info!(
            quantos = paths.len(),
            "cópia de dentro da pasta: vão os caminhos, e não os bytes"
        );
        saida.enviar(FolderMessage::Copied {
            folder: pasta,
            paths,
        });
    }

    /// O outro computador copiou arquivos da pasta: os mesmos caminhos, na cópia daqui, vão ao
    /// clipboard deste computador. Só os que existem aqui.
    pub(super) fn copiado_la(&mut self, pasta: FolderId, paths: &[String]) {
        let Some(viva) = self.vivas.get(&pasta) else {
            return;
        };
        let locais: Vec<String> = paths
            .iter()
            .filter(|caminho| is_safe_relative_path(caminho))
            .map(|caminho| crate::disco::absoluto(viva.visivel(), caminho))
            .filter(|caminho| std::fs::symlink_metadata(caminho).is_ok())
            .map(|caminho| caminho.to_string_lossy().into_owned())
            .collect();
        if !locais.is_empty() {
            info!(
                quantos = locais.len(),
                "o outro computador copiou da pasta: no clipboard daqui"
            );
            self.para_o_clipboard = Some(locais);
        }
    }

    /// O que o outro computador copiou da pasta, para o clipboard daqui — uma vez.
    pub fn tirar_do_clipboard(&mut self) -> Option<Vec<String>> {
        self.para_o_clipboard.take()
    }
}
