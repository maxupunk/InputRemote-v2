//! A entrada desta máquina: ler o teclado e o mouse daqui, e receber os do outro.
//!
//! Sem papel fixo ([ADR-0014](../../../../docs/adr/0014-controle-simetrico.md)), toda máquina faz as
//! duas coisas, e as duas ficam abertas o tempo todo. No Windows as duas são do agente, na sessão do
//! usuário; no Linux, do serviço — a captura por `evdev` e a injeção por `uinput`, peças separadas
//! que podem faltar uma sem a outra.

#[cfg(not(windows))]
use tracing::{info, warn};

use super::Daemon;

/// O que está aberto: ler o teclado daqui, e receber o do outro.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct EntradaLocal {
    /// Lê o teclado e o mouse ligados a esta máquina, para controlar o outro.
    pub(crate) le: bool,
    /// Recebe o teclado e o mouse do outro.
    pub(crate) recebe: bool,
}

impl Daemon {
    /// Abre o que faltar, e diz o que está aberto.
    #[cfg(not(windows))]
    pub(crate) fn garantir_entrada_local(&mut self) -> EntradaLocal {
        if self.capturer.is_none() {
            match crate::fundo::capturar(&self.captura) {
                Ok(capturador) => {
                    info!("captura ligada: o teclado e o mouse daqui controlam o outro");
                    self.capturer = Some(capturador);
                }
                Err(erro) => warn!(%erro, "captura local indisponível"),
            }
        }
        if self.injector.is_none() {
            match ir_input::open_injector() {
                Ok(injetor) => {
                    info!("injetor aberto");
                    self.injector = Some(injetor);
                }
                Err(erro) => warn!(%erro, "injeção local indisponível"),
            }
        }
        self.ajustar_conducao();
        EntradaLocal {
            le: self.capturer.is_some(),
            recebe: self.injector.is_some(),
        }
    }

    /// No Windows quem captura e injeta é o agente.
    #[cfg(windows)]
    #[allow(clippy::unused_self)]
    pub(crate) const fn garantir_entrada_local(&mut self) -> EntradaLocal {
        EntradaLocal {
            le: true,
            recebe: true,
        }
    }

    /// Com a captura parada, tenta de novo — em silêncio até conseguir.
    ///
    /// A captura que falhou na subida (o teclado USB que ainda não tinha aparecido, uma permissão
    /// dada depois) ficava parada até reiniciar o serviço. Agora a máquina se recupera sozinha, e a
    /// tela sai do aviso quando a captura sobe (log 47).
    #[cfg(not(windows))]
    pub(super) fn recuperar_captura(&mut self) {
        if !self.session.policy().sends() || self.capturer.is_some() {
            return;
        }
        match crate::fundo::capturar(&self.captura) {
            Ok(capturador) => {
                info!("captura ligada: este computador voltou a poder controlar o outro");
                self.capturer = Some(capturador);
                self.ajustar_conducao();
                let _ = self.avisos.send(ir_ipc::Aviso::EstadoMudou(self.estado()));
            }
            Err(erro) => tracing::debug!(%erro, "a captura ainda não abre"),
        }
    }

    /// No Windows quem captura é o agente, e quem o traz de volta é o laço do agente.
    #[cfg(windows)]
    #[allow(clippy::unused_self)]
    pub(super) const fn recuperar_captura(&mut self) {}
}
