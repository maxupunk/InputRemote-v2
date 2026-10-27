//! As fontes juntas numa amostra.

use std::time::Instant;

use ir_monitor::amostra::UsageSample;

use crate::fonte::Agendada;

/// Junta as fontes numa amostra, uma batida de cada vez.
///
/// Guarda a amostra entre as batidas: a fonte que não leu nesta batida deixa o último valor no
/// lugar. Um coletor novo começa vazio — é o que faz uma assinatura nova não mostrar número de
/// uma assinatura velha.
#[derive(Debug, Default)]
pub struct Coletor {
    fontes: Vec<Agendada>,
    amostra: UsageSample,
}

impl Coletor {
    /// Um coletor com estas fontes.
    #[must_use]
    pub fn novo(fontes: Vec<Agendada>) -> Self {
        Self {
            fontes,
            amostra: UsageSample::default(),
        }
    }

    /// Uma batida: cada fonte lê se chegou a vez dela, e sai a amostra de agora.
    pub fn amostrar(&mut self, agora: Instant) -> UsageSample {
        for fonte in &mut self.fontes {
            fonte.bater(agora, &mut self.amostra);
        }
        self.amostra.clone().bounded()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fonte::Fonte;

    struct Cpu(u16);

    impl Fonte for Cpu {
        fn ler(&mut self, _: Instant, amostra: &mut UsageSample) {
            amostra.cpu_permille = Some(self.0);
            self.0 += 100;
        }
    }

    #[test]
    fn entre_leituras_o_campo_guarda_o_ultimo_valor() {
        let mut coletor = Coletor::novo(vec![Agendada::nova(Cpu(100), 2)]);
        let agora = Instant::now();
        assert_eq!(coletor.amostrar(agora).cpu_permille, Some(100));
        assert_eq!(coletor.amostrar(agora).cpu_permille, Some(100));
        assert_eq!(coletor.amostrar(agora).cpu_permille, Some(200));
    }

    #[test]
    fn sem_fonte_a_amostra_sai_vazia() {
        let mut coletor = Coletor::novo(Vec::new());
        assert_eq!(coletor.amostrar(Instant::now()), UsageSample::default());
    }

    #[test]
    fn a_amostra_sai_dentro_dos_tetos() {
        let mut coletor = Coletor::novo(vec![Agendada::nova(Cpu(5000), 1)]);
        assert_eq!(coletor.amostrar(Instant::now()).cpu_permille, Some(1000));
    }
}
