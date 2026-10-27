//! Uma métrica, e de quanto em quanto tempo ela é lida.

use std::time::Instant;

use ir_monitor::amostra::UsageSample;

/// Uma fonte lê uma coisa só e escreve o campo dela na amostra.
///
/// Quem não consegue ler deixa o campo como está: o coletor começa cada assinatura com a amostra
/// vazia, então "não consegui" vira "—" na tela, e não zero.
pub trait Fonte {
    /// Lê agora e escreve o resultado na amostra.
    fn ler(&mut self, agora: Instant, amostra: &mut UsageSample);
}

/// Uma fonte e a sua cadência, em batidas de um segundo.
///
/// O processador e a rede mudam a cada segundo; a temperatura e a bateria, não. Ler cada uma no
/// ritmo dela é o que segura o custo da consulta mais cara, a do WMI ([12, §3.3]). Entre uma
/// leitura e outra o campo continua com o último valor.
///
/// [12, §3.3]: ../../../docs/12-painel-android.md
pub struct Agendada {
    fonte: Box<dyn Fonte>,
    cada: u32,
    falta: u32,
}

impl core::fmt::Debug for Agendada {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Agendada")
            .field("cada", &self.cada)
            .field("falta", &self.falta)
            .finish_non_exhaustive()
    }
}

impl Agendada {
    /// Lida a cada `cada` batidas; `0` vale como `1`. A primeira batida sempre lê.
    #[must_use]
    pub fn nova(fonte: impl Fonte + 'static, cada: u32) -> Self {
        Self {
            fonte: Box::new(fonte),
            cada: cada.max(1),
            falta: 0,
        }
    }

    /// Uma batida: lê se chegou a vez.
    pub fn bater(&mut self, agora: Instant, amostra: &mut UsageSample) {
        if self.falta == 0 {
            self.fonte.ler(agora, amostra);
            self.falta = self.cada;
        }
        self.falta -= 1;
    }
}

/// As cadências, num lugar só. Decisão de 2026-10-24 ([12, §3.3](../../../docs/12-painel-android.md)).
pub mod cadencia {
    /// Processador, memória e rede: a cada batida.
    pub const RAPIDA: u32 = 1;
    /// Temperatura e bateria: a cada 5 s.
    pub const MEDIA: u32 = 5;
    /// Disco: a cada 30 s.
    pub const LENTA: u32 = 30;
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    use std::rc::Rc;

    struct Contadora(Rc<Cell<u32>>);

    impl Fonte for Contadora {
        fn ler(&mut self, _: Instant, _: &mut UsageSample) {
            self.0.set(self.0.get() + 1);
        }
    }

    #[test]
    fn le_na_primeira_batida_e_depois_a_cada_n() {
        let lidas = Rc::new(Cell::new(0));
        let mut agendada = Agendada::nova(Contadora(Rc::clone(&lidas)), 5);
        let mut amostra = UsageSample::default();
        let agora = Instant::now();
        for _ in 0..11 {
            agendada.bater(agora, &mut amostra);
        }
        // Batidas 0, 5 e 10.
        assert_eq!(lidas.get(), 3);
    }

    #[test]
    fn cadencia_zero_vale_como_toda_batida() {
        let lidas = Rc::new(Cell::new(0));
        let mut agendada = Agendada::nova(Contadora(Rc::clone(&lidas)), 0);
        let mut amostra = UsageSample::default();
        for _ in 0..3 {
            agendada.bater(Instant::now(), &mut amostra);
        }
        assert_eq!(lidas.get(), 3);
    }
}
