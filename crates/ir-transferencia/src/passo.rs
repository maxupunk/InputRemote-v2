//! Quando contar o andamento de uma cópia.
//!
//! Antes só se contava no fim de cada arquivo. Uma pasta com um arquivo de 2 GB ficava em **0% o
//! tempo todo** e pulava para 100% — o usuário via um aviso parado e concluía que nada estava
//! acontecendo. Foi o que ele relatou.
//!
//! Contar a cada bloco seria o outro extremo: são dezenas de milhares por segundo, e cada aviso
//! atravessa o canal de controle até a janela. O meio é o tempo: um aviso a cada fração de segundo,
//! que é a velocidade em que um número na tela ainda é legível — mais que isso vira borrão.
//!
//! O fim **sempre** passa, tenha ou não passado o intervalo: é o aviso que fecha a cópia na tela.

use std::time::{Duration, Instant};

/// De quanto em quanto tempo o andamento é contado.
///
/// Cinco avisos por segundo: o bastante para a barra andar e o número mudar sem piscar, e pouco
/// para não pesar no canal de controle nem no registro.
const INTERVALO: Duration = Duration::from_millis(200);

/// O relógio que decide se já é hora de contar.
#[derive(Debug)]
pub(crate) struct Passo {
    ultimo: Instant,
}

impl Passo {
    /// Um relógio que deixa o primeiro andamento passar.
    pub(crate) fn novo() -> Self {
        // Um intervalo atrás: o primeiro bloco já conta, e a cópia aparece na tela assim que
        // começa, em vez de só no segundo tique. Logo depois de a máquina ligar não há "um
        // intervalo atrás", e aí vale agora — o primeiro aviso sai 200 ms depois, e ninguém nota.
        Self::novo_em(Instant::now())
    }

    /// O mesmo, com o relógio dado — é assim que isto é testado sem esperar de verdade.
    fn novo_em(agora: Instant) -> Self {
        Self {
            ultimo: agora.checked_sub(INTERVALO).unwrap_or(agora),
        }
    }

    /// Se é hora de contar o andamento agora.
    pub(crate) fn passou(&mut self) -> bool {
        self.passou_em(Instant::now())
    }

    /// O mesmo, com o relógio dado — é assim que isto é testado sem esperar de verdade.
    fn passou_em(&mut self, agora: Instant) -> bool {
        if agora.duration_since(self.ultimo) < INTERVALO {
            return false;
        }
        self.ultimo = agora;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn o_primeiro_andamento_passa_na_hora() {
        let inicio = Instant::now();
        let mut passo = Passo::novo_em(inicio);
        assert!(passo.passou_em(inicio), "a cópia aparece ao começar");
    }

    #[test]
    fn dentro_do_intervalo_nao_conta_de_novo() {
        let inicio = Instant::now();
        let mut passo = Passo::novo_em(inicio);
        assert!(passo.passou_em(inicio));
        assert!(!passo.passou_em(inicio + Duration::from_millis(50)));
        assert!(!passo.passou_em(inicio + Duration::from_millis(199)));
    }

    #[test]
    fn passado_o_intervalo_conta_de_novo() {
        let inicio = Instant::now();
        let mut passo = Passo::novo_em(inicio);
        assert!(passo.passou_em(inicio));
        assert!(passo.passou_em(inicio + INTERVALO));
        assert!(!passo.passou_em(inicio + INTERVALO + Duration::from_millis(1)));
        assert!(passo.passou_em(inicio + INTERVALO * 2));
    }

    #[test]
    fn um_despejo_de_dez_mil_blocos_nao_vira_dez_mil_avisos() {
        let inicio = Instant::now();
        let mut passo = Passo::novo_em(inicio);
        let mut avisos = 0;
        // Dez segundos de blocos, cem por segundo.
        for i in 0..1000 {
            if passo.passou_em(inicio + Duration::from_millis(i * 10)) {
                avisos += 1;
            }
        }
        // Dez segundos a cinco avisos por segundo. O último bloco cai em 9,99 s, então o
        // quinquagésimo primeiro aviso ficaria para 10,0 s, que já é depois do fim.
        assert_eq!(avisos, 50, "cinco por segundo");
    }
}
