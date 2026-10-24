//! As contas entre duas leituras: o sistema dá contadores que só crescem, e a tela quer taxa.

use std::time::{Duration, Instant};

/// Os tempos acumulados do processador: quanto ficou ocioso, de quanto passou.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TemposDoProcessador {
    /// Tempo ocioso acumulado, em qualquer unidade.
    pub ocioso: u64,
    /// Tempo total acumulado, na mesma unidade.
    pub total: u64,
}

/// O uso do processador entre duas leituras, em milésimos.
///
/// `None` sem leitura anterior, ou se o total não andou — duas leituras no mesmo instante, ou
/// um contador que voltou (a máquina acordou, um núcleo saiu).
#[must_use]
pub fn uso_do_processador(antes: TemposDoProcessador, agora: TemposDoProcessador) -> Option<u16> {
    let total = agora.total.checked_sub(antes.total).filter(|t| *t > 0)?;
    let ocioso = agora.ocioso.checked_sub(antes.ocioso)?.min(total);
    let ocupado = u128::from(total - ocioso);
    u16::try_from(ocupado * 1000 / u128::from(total)).ok()
}

/// Um contador de bytes que vira taxa por segundo.
#[derive(Debug, Default)]
pub struct Taxa {
    anterior: Option<(Instant, u64)>,
}

impl Taxa {
    /// Anota a leitura de agora e devolve os bytes por segundo desde a anterior.
    ///
    /// `None` na primeira leitura, ou se o contador voltou (a interface caiu e subiu de novo). A
    /// próxima leitura parte desta, então um contador que voltou custa uma amostra só.
    pub fn anotar(&mut self, agora: Instant, bytes: u64) -> Option<u64> {
        let anterior = self.anterior.replace((agora, bytes));
        let (quando, antes) = anterior?;
        let passou = agora.saturating_duration_since(quando);
        let delta = bytes.checked_sub(antes)?;
        por_segundo(delta, passou)
    }
}

fn por_segundo(delta: u64, passou: Duration) -> Option<u64> {
    let micros = passou.as_micros();
    if micros == 0 {
        return None;
    }
    u64::try_from(u128::from(delta) * 1_000_000 / micros).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    const fn tempos(ocioso: u64, total: u64) -> TemposDoProcessador {
        TemposDoProcessador { ocioso, total }
    }

    #[test]
    fn um_quarto_ocupado_sao_250_milesimos() {
        assert_eq!(
            uso_do_processador(tempos(100, 1000), tempos(400, 1400)),
            Some(250)
        );
    }

    #[test]
    fn total_parado_ou_contador_que_voltou_nao_tem_uso() {
        assert_eq!(uso_do_processador(tempos(1, 10), tempos(1, 10)), None);
        assert_eq!(uso_do_processador(tempos(1, 10), tempos(1, 5)), None);
    }

    #[test]
    fn ocioso_maior_que_o_total_satura_em_zero_uso() {
        assert_eq!(uso_do_processador(tempos(0, 0), tempos(50, 10)), Some(0));
    }

    #[test]
    fn a_taxa_divide_pelo_tempo_que_passou() {
        let inicio = Instant::now();
        let mut taxa = Taxa::default();
        assert_eq!(taxa.anotar(inicio, 1_000), None);
        let depois = inicio + Duration::from_millis(500);
        assert_eq!(taxa.anotar(depois, 2_000), Some(2_000));
    }

    #[test]
    fn contador_que_voltou_custa_uma_amostra() {
        let inicio = Instant::now();
        let mut taxa = Taxa::default();
        taxa.anotar(inicio, 5_000);
        let um = inicio + Duration::from_secs(1);
        assert_eq!(taxa.anotar(um, 10), None);
        let dois = um + Duration::from_secs(1);
        assert_eq!(taxa.anotar(dois, 110), Some(100));
    }
}
