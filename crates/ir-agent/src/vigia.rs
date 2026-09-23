//! O vigia da supressão: o teclado do usuário não fica morto se o serviço travar.
//!
//! Com a supressão ligada, o teclado e o mouse desta máquina só alimentam o par. Se o serviço
//! trava nesse estado — sem cair, então sem fechar o canal —, nada mais desliga a supressão, e o
//! usuário fica sem teclado e sem mouse na própria máquina. Enquanto suprime, o serviço renova o
//! pedido a cada segundo (o vigia espera [`PRAZO`]); o vigia devolve a entrada local quando a renovação
//! para de chegar.

use std::time::{Duration, Instant};

use ir_ipc::ComandoDoAgente;

/// Quanto tempo sem renovação até o vigia devolver a entrada local.
///
/// Três renovações perdidas: um soluço do serviço não solta o controle no meio do uso, e um
/// serviço travado devolve o teclado em poucos segundos.
pub(crate) const PRAZO: Duration = Duration::from_secs(3);

/// O que o vigia sabe da supressão.
#[derive(Debug, Default)]
pub(crate) struct Vigia {
    /// Desde quando a supressão foi pedida pela última vez, se está ligada.
    renovada: Option<Instant>,
}

impl Vigia {
    /// Anota um comando do serviço.
    pub(crate) fn viu(&mut self, comando: ComandoDoAgente, agora: Instant) {
        if let ComandoDoAgente::SuprimirEntradaLocal(ligada) = comando {
            self.renovada = ligada.then_some(agora);
        }
    }

    /// Se a supressão venceu sem renovação. Devolve `true` uma vez só, e esquece a supressão.
    pub(crate) fn venceu(&mut self, agora: Instant) -> bool {
        match self.renovada {
            Some(desde) if agora.saturating_duration_since(desde) > PRAZO => {
                self.renovada = None;
                true
            }
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_supressao_renovada_nao_vence() {
        let inicio = Instant::now();
        let mut vigia = Vigia::default();
        vigia.viu(ComandoDoAgente::SuprimirEntradaLocal(true), inicio);
        for segundo in 1..10 {
            let agora = inicio + Duration::from_secs(segundo);
            vigia.viu(ComandoDoAgente::SuprimirEntradaLocal(true), agora);
            assert!(!vigia.venceu(agora + Duration::from_millis(900)));
        }
    }

    #[test]
    fn o_servico_calado_devolve_a_entrada_uma_vez() {
        let inicio = Instant::now();
        let mut vigia = Vigia::default();
        vigia.viu(ComandoDoAgente::SuprimirEntradaLocal(true), inicio);
        assert!(!vigia.venceu(inicio + PRAZO));
        assert!(vigia.venceu(inicio + PRAZO + Duration::from_millis(1)));
        assert!(
            !vigia.venceu(inicio + PRAZO * 2),
            "devolvida uma vez, não há o que devolver de novo"
        );
    }

    #[test]
    fn sem_supressao_nao_ha_o_que_vencer() {
        let inicio = Instant::now();
        let mut vigia = Vigia::default();
        assert!(!vigia.venceu(inicio + PRAZO * 10));
        vigia.viu(ComandoDoAgente::SuprimirEntradaLocal(true), inicio);
        vigia.viu(ComandoDoAgente::SuprimirEntradaLocal(false), inicio);
        assert!(!vigia.venceu(inicio + PRAZO * 10));
    }
}
