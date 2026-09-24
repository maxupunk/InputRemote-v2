//! A aceleração do ponteiro, para o cursor que o serviço conduz ter a sensação do de sempre.
//!
//! Com o controle aqui, o serviço toma o mouse e o touchpad e move o cursor ele mesmo (log 50). Os
//! deslocamentos crus de um mouse andam devagar demais sem a aceleração que o compositor aplicaria:
//! movimento lento é preciso, rápido atravessa a tela. Esta é uma curva pequena no espírito da
//! `libinput` — fator 1 devagar, crescendo com a velocidade, com teto.
//!
//! Sem E/S: recebe o deslocamento e o instante do evento, devolve o deslocamento acelerado.

use std::time::Duration;

/// Abaixo desta velocidade, em unidades por milissegundo, o movimento não é acelerado.
const LIMIAR: f32 = 0.4;

/// Quanto o fator cresce por unidade de velocidade acima do limiar.
const INCLINACAO: f32 = 0.7;

/// O maior fator.
const TETO: f32 = 4.0;

/// O maior intervalo que ainda conta como o mesmo movimento. Depois de uma pausa, o primeiro
/// deslocamento não é tratado como rápido.
const PAUSA: Duration = Duration::from_millis(50);

/// O estado da aceleração de um dispositivo.
#[derive(Debug, Default)]
pub(super) struct Acelerador {
    /// Quando veio o deslocamento anterior.
    anterior: Option<Duration>,
    /// A fração de pixel que sobrou, para o movimento lento não sumir.
    resto: (f32, f32),
}

impl Acelerador {
    /// O deslocamento acelerado, dado o instante do evento (desde qualquer origem fixa).
    pub(super) fn aplicar(&mut self, dx: i32, dy: i32, quando: Duration) -> (i32, i32) {
        let intervalo = self
            .anterior
            .replace(quando)
            .and_then(|antes| quando.checked_sub(antes))
            .filter(|dt| *dt <= PAUSA);
        #[allow(clippy::cast_precision_loss)]
        let (fx, fy) = (dx as f32, dy as f32);
        let fator = intervalo.map_or(1.0, |dt| {
            let ms = (dt.as_secs_f32() * 1000.0).max(1.0);
            let velocidade = fx.hypot(fy) / ms;
            (INCLINACAO.mul_add((velocidade - LIMIAR).max(0.0), 1.0)).min(TETO)
        });
        self.resto.0 += fx * fator;
        self.resto.1 += fy * fator;
        let (ix, iy) = (self.resto.0.trunc(), self.resto.1.trunc());
        self.resto.0 -= ix;
        self.resto.1 -= iy;
        #[allow(clippy::cast_possible_truncation)]
        (ix as i32, iy as i32)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    #[test]
    fn devagar_nao_acelera() {
        let mut a = Acelerador::default();
        assert_eq!(
            a.aplicar(1, 0, ms(0)),
            (1, 0),
            "o primeiro não tem velocidade"
        );
        // 1 unidade a cada 8 ms: 0,125 por ms, abaixo do limiar.
        assert_eq!(a.aplicar(1, 0, ms(8)), (1, 0));
        assert_eq!(a.aplicar(0, -1, ms(16)), (0, -1));
    }

    #[test]
    fn rapido_acelera_ate_o_teto() {
        let mut a = Acelerador::default();
        a.aplicar(0, 0, ms(0));
        // 10 unidades em 1 ms: bem acima do limiar, fator no teto.
        let (dx, _) = a.aplicar(10, 0, ms(1));
        assert_eq!(dx, 40);
        // Moderado: 4 em 2 ms é 2 por ms, fator 1 + 0,7 × 1,6 = 2,12.
        let (dx, _) = a.aplicar(4, 0, ms(3));
        assert_eq!(dx, 8);
    }

    #[test]
    fn depois_de_uma_pausa_recomeca_sem_acelerar() {
        let mut a = Acelerador::default();
        a.aplicar(10, 0, ms(0));
        assert_eq!(a.aplicar(10, 0, ms(500)), (10, 0));
    }

    #[test]
    fn a_fracao_nao_se_perde() {
        let mut a = Acelerador::default();
        a.aplicar(0, 0, ms(0));
        // 3 em 2 ms: 1,5 por ms, fator 1,77 → 5,31: sai 5, sobra 0,31, que soma na próxima.
        assert_eq!(a.aplicar(3, 0, ms(2)), (5, 0));
        assert_eq!(a.aplicar(3, 0, ms(4)), (5, 0));
        assert_eq!(a.aplicar(3, 0, ms(6)), (5, 0));
        assert_eq!(
            a.aplicar(3, 0, ms(8)),
            (6, 0),
            "0,31 × 4 passou de um pixel"
        );
    }
}
