//! O resto da roda entre um evento e outro.
//!
//! A roda anda em passos inteiros — marcações no `REL_WHEEL` do `uinput`, unidades de 1/120 de
//! marcação no protocolo —, mas o que chega pode ser menos que um passo: o touchpad de precisão do
//! Windows manda roda fina, abaixo de 120, e o dedo no touchpad do Linux anda frações de pixel.
//! Dividir e jogar fora o resto fazia a rolagem lenta sumir inteira. Aqui o resto fica guardado,
//! por eixo, até completar um passo.
//!
//! O resto **não** zera quando a direção inverte: é o comportamento que o touchpad já tinha, e a
//! inversão só devolve a fração que tinha sobrado.

/// O que sobrou de roda nos dois eixos, em unidades de entrada.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct AcumuladorDeRoda {
    /// Quantas unidades de entrada fazem um passo.
    passo: f32,
    /// O que sobrou, horizontal e vertical, sempre menor que um passo em módulo.
    resto: (f32, f32),
}

impl AcumuladorDeRoda {
    /// Um acumulador em que `passo` unidades de entrada fazem um passo de saída.
    pub(crate) const fn novo(passo: f32) -> Self {
        Self {
            passo,
            resto: (0.0, 0.0),
        }
    }

    /// Soma o que chegou e devolve os passos inteiros que se completaram, horizontal e vertical.
    ///
    /// Os passos vão em direção a zero: o resto guarda o sinal do movimento, e o próximo evento no
    /// mesmo sentido o completa.
    #[doc = "hot path"]
    pub(crate) fn acumular(&mut self, dx: f32, dy: f32) -> (i32, i32) {
        (
            passos(&mut self.resto.0, dx, self.passo),
            passos(&mut self.resto.1, dy, self.passo),
        )
    }
}

/// Soma `quanto` ao resto de um eixo e tira dele os passos inteiros.
fn passos(resto: &mut f32, quanto: f32, passo: f32) -> i32 {
    *resto += quanto;
    let inteiros = (*resto / passo).trunc();
    *resto -= inteiros * passo;
    #[allow(clippy::cast_possible_truncation)]
    let inteiros = inteiros as i32;
    inteiros
}

#[cfg(test)]
#[allow(clippy::float_cmp)]
mod tests {
    use super::*;

    #[test]
    fn a_roda_fina_se_soma_ate_completar_uma_marcacao() {
        // Regressão: `dy / 120` sem resto jogava fora toda rolagem do touchpad de precisão.
        let mut roda = AcumuladorDeRoda::novo(120.0);
        assert_eq!(roda.acumular(0.0, 40.0), (0, 0));
        assert_eq!(roda.acumular(0.0, 40.0), (0, 0));
        assert_eq!(roda.acumular(0.0, 40.0), (0, 1));
        assert_eq!(roda.acumular(0.0, 0.0), (0, 0), "nada sobrou");
    }

    #[test]
    fn marcacoes_inteiras_passam_na_hora() {
        let mut roda = AcumuladorDeRoda::novo(120.0);
        assert_eq!(roda.acumular(-240.0, 360.0), (-2, 3));
        assert_eq!(roda.acumular(-60.0, -60.0), (0, 0));
        assert_eq!(roda.acumular(-60.0, -60.0), (-1, -1));
    }

    #[test]
    fn os_eixos_nao_se_misturam() {
        let mut roda = AcumuladorDeRoda::novo(120.0);
        assert_eq!(roda.acumular(100.0, 0.0), (0, 0));
        assert_eq!(roda.acumular(0.0, 100.0), (0, 0));
        assert_eq!(roda.acumular(20.0, 0.0), (1, 0));
        assert_eq!(roda.acumular(0.0, 20.0), (0, 1));
    }

    #[test]
    fn inverter_devolve_so_o_que_sobrou() {
        // Sem zerar na inversão, como o touchpad sempre fez: 100 para cima e 100 para baixo é zero.
        let mut roda = AcumuladorDeRoda::novo(120.0);
        assert_eq!(roda.acumular(0.0, 100.0), (0, 0));
        assert_eq!(roda.acumular(0.0, -100.0), (0, 0));
        assert_eq!(roda.resto, (0.0, 0.0));
        assert_eq!(roda.acumular(0.0, -120.0), (0, -1));
    }

    #[test]
    fn passo_fracionario_para_o_touchpad() {
        // Meio pixel por unidade de roda: 1,25 px dá duas unidades e sobra um quarto de pixel.
        let mut roda = AcumuladorDeRoda::novo(0.5);
        assert_eq!(roda.acumular(0.0, 1.25), (0, 2));
        assert_eq!(roda.acumular(0.0, 0.25), (0, 1));
    }
}
