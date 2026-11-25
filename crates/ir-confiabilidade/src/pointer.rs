//! O filtro do canal 2: cada amostra de ponteiro vale **uma vez**, chegue quando chegar.
//!
//! O movimento é relativo. Aplicar a mesma amostra duas vezes move o cursor o dobro — na rota
//! dupla toda amostra chega duas vezes —, e deixar de aplicar uma faz o cursor andar menos do que
//! a mão andou.
//!
//! O filtro de antes aceitava só a amostra mais nova que a última aplicada. Na rota dupla isso
//! perdia movimento justamente quando a redundância devia salvá-lo: a cópia da rede se perde, a
//! do Bluetooth chega dez ou vinte milissegundos depois, e nesse meio a amostra seguinte já veio
//! pela rede — a cópia que salvaria o movimento era descartada como velha. Com o Wi-Fi perdendo
//! quadros, o cursor do outro lado ficava "lento".
//!
//! Agora é uma janela de repetição, como a do transporte: a mais nova avança a janela, uma
//! atrasada que ainda cabe nela e nunca foi vista também passa, e só a repetida — ou velha demais
//! para saber — é descartada. Somar deslocamentos fora de ordem leva ao mesmo lugar.

use ir_proto::frame::Sequence;

/// Quantas amostras para trás a janela lembra. Com uma amostra a cada poucos milissegundos, é
/// bem mais que a diferença de atraso entre os dois portadores.
const JANELA: u32 = 64;

/// Como uma amostra chegou, em relação às já aplicadas.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Arrival {
    /// Mais nova que todas: aplicar.
    Newest,
    /// Atrasada, mas nunca vista: aplicar, se for movimento relativo.
    Late,
    /// Já aplicada, ou velha demais para saber: descartar.
    Repeated,
}

/// A janela de amostras já aplicadas.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PointerFilter {
    /// A sequência mais nova aplicada.
    newest: Option<Sequence>,
    /// Bit `k` = a sequência `newest - k` já foi aplicada. O bit 0 é a própria `newest`.
    seen: u64,
}

impl PointerFilter {
    /// Classifica a amostra e, se ela for nova, a marca como aplicada.
    #[must_use = "uma amostra classificada e ignorada já foi marcada como aplicada"]
    pub fn admit(&mut self, seq: Sequence) -> Arrival {
        let Some(newest) = self.newest else {
            self.newest = Some(seq);
            self.seen = 1;
            return Arrival::Newest;
        };
        if seq.is_newer_than(newest) {
            let ahead = seq.distance_from(newest);
            self.seen = if ahead >= JANELA {
                0
            } else {
                self.seen << ahead
            } | 1;
            self.newest = Some(seq);
            return Arrival::Newest;
        }
        let behind = newest.distance_from(seq);
        if behind >= JANELA {
            return Arrival::Repeated;
        }
        let bit = 1u64 << behind;
        if self.seen & bit != 0 {
            return Arrival::Repeated;
        }
        self.seen |= bit;
        Arrival::Late
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn admit(filter: &mut PointerFilter, seq: u32) -> Arrival {
        filter.admit(Sequence(seq))
    }

    #[test]
    fn each_sample_counts_once_in_order() {
        let mut f = PointerFilter::default();
        for seq in 1..=200 {
            assert_eq!(admit(&mut f, seq), Arrival::Newest);
            assert_eq!(
                admit(&mut f, seq),
                Arrival::Repeated,
                "a cópia da rota dupla"
            );
        }
    }

    #[test]
    fn a_late_copy_of_a_lost_sample_still_counts() {
        // A rede perdeu a 2; a 3 veio por ela, e a cópia da 2 pelo Bluetooth chega depois.
        let mut f = PointerFilter::default();
        assert_eq!(admit(&mut f, 1), Arrival::Newest);
        assert_eq!(admit(&mut f, 3), Arrival::Newest);
        assert_eq!(
            admit(&mut f, 2),
            Arrival::Late,
            "o movimento da 2 não se perde"
        );
        assert_eq!(admit(&mut f, 2), Arrival::Repeated);
        assert_eq!(admit(&mut f, 3), Arrival::Repeated);
    }

    #[test]
    fn too_old_to_know_is_dropped() {
        let mut f = PointerFilter::default();
        assert_eq!(admit(&mut f, 100), Arrival::Newest);
        assert_eq!(admit(&mut f, 100 - JANELA), Arrival::Repeated);
        assert_eq!(admit(&mut f, 100 - JANELA + 1), Arrival::Late);
    }

    #[test]
    fn a_jump_past_the_window_forgets_the_old_ones() {
        let mut f = PointerFilter::default();
        assert_eq!(admit(&mut f, 1), Arrival::Newest);
        assert_eq!(admit(&mut f, 1 + JANELA * 3), Arrival::Newest);
        assert_eq!(admit(&mut f, 1), Arrival::Repeated);
        assert_eq!(admit(&mut f, JANELA * 3), Arrival::Late);
    }

    #[test]
    fn the_sequence_wraps_without_freezing() {
        let mut f = PointerFilter::default();
        assert_eq!(admit(&mut f, u32::MAX - 1), Arrival::Newest);
        assert_eq!(admit(&mut f, 1), Arrival::Newest, "deu a volta");
        assert_eq!(admit(&mut f, u32::MAX), Arrival::Late);
        assert_eq!(admit(&mut f, 0), Arrival::Late);
        assert_eq!(admit(&mut f, u32::MAX - 1), Arrival::Repeated);
    }
}
