//! A conversa com o par sobre o painel: o pedido da amostra dele, e o que ele pede daqui.
//!
//! Vai dentro de `Control::Panel`, opaco para a sessão ([`ir_monitor::EntrePcs`]).

use std::time::Instant;

use ir_monitor::EntrePcs;
use ir_monitor::entre_pcs::{codificar, decodificar};
use ir_session::Input;

use super::{PEDIDO_DO_PAR_EXPIRA, RENOVAR_A_CADA, Vigilancia};
use crate::Sessao;

/// O recado em entrada para a sessão; `None` se não coube.
pub(super) fn mandar(recado: &EntrePcs) -> Option<Input> {
    codificar(recado).map(Input::Panel)
}

impl Vigilancia {
    /// Um recado do par: o pedido dele, ou a amostra que ele mandou.
    pub(super) fn recado_do_par(&mut self, agora: Instant, bytes: &[u8]) {
        match decodificar(bytes) {
            Some(EntrePcs::Assinar(interesse)) => {
                self.par_quer = Some((agora + PEDIDO_DO_PAR_EXPIRA, interesse));
                self.ajustar_vigia();
            }
            Some(EntrePcs::Parar) => {
                self.par_quer = None;
                self.ajustar_vigia();
            }
            Some(EntrePcs::Uso(amostra, processos)) => {
                self.mesa.anotar_par(agora, amostra);
                if let Some(processos) = processos {
                    self.mesa.anotar_processos_do_par(agora, processos);
                }
            }
            // De um par mais novo, com vocabulário que este não conhece: ignora, sem derrubar.
            None => tracing::debug!("recado do painel do par não entendido"),
        }
    }

    /// Com painel assinando e par que entende, pede a amostra dele — renovando, porque ele a
    /// esquece sem renovação, e na hora quando os processos pedidos mudam. Sem assinante, desfaz o
    /// pedido uma vez.
    pub(super) fn pedir_ao_par(&mut self, sessao: &impl Sessao) -> Vec<Input> {
        let entende = sessao.par().is_some_and(|(_, entende)| entende);
        let quer = (self.assinantes > 0 && entende).then(|| self.dos_paineis[1].clone());
        let recado = match (&quer, &self.pedido) {
            (Some(agora), Some(antes))
                if agora == antes && !self.batidas.is_multiple_of(RENOVAR_A_CADA) =>
            {
                None
            }
            (Some(interesse), _) => Some(EntrePcs::Assinar(interesse.clone())),
            (None, Some(_)) if entende => Some(EntrePcs::Parar),
            (None, _) => None,
        };
        self.pedido = quer;
        recado.as_ref().and_then(mandar).into_iter().collect()
    }
}
