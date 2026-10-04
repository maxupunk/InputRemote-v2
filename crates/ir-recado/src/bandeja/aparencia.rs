//! Como o ícone da bandeja fica, pelo que está acontecendo.
//!
//! O ícone ao lado do relógio é o único pedaço do InputRemote sempre à vista. Ele responde "está
//! acontecendo alguma coisa?" sem abrir nada, com um selo no canto, como o do OneDrive:
//!
//! | O que acontece | O ícone |
//! |---|---|
//! | uma cópia anda, ou uma pasta sincroniza | o arco girando |
//! | uma cópia não atravessou | o ! vermelho, até a pessoa abrir a janela ou copiar de novo |
//! | uma cópia chegou ou foi entregue | o ✓ verde, por alguns segundos |
//! | sem o outro computador, ou pausado | cinza |
//! | o resto | o ícone de sempre |
//!
//! O ! fica porque a falha é a única coisa que pede ação de quem copiou, e a notificação some
//! sozinha; o ✓ some porque sucesso não pede nada.
//!
//! Separado do desenho para ser testado: aqui só se decide, com o relógio de fora.

use std::time::{Duration, Instant};

use crate::Tom;

/// Quanto tempo o ✓ fica depois de uma cópia dar certo.
const FEITO: Duration = Duration::from_secs(4);

/// Como o ícone fica.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Aparencia {
    /// O ícone de sempre.
    Normal,
    /// Cinza: sem o outro computador, ou pausado.
    Inativo,
    /// O ✓: uma cópia acabou de dar certo.
    Feito,
    /// O !: uma cópia não atravessou, e a pessoa ainda não viu.
    Problema,
    /// O arco girando: algo atravessa agora.
    Trabalhando,
}

/// O que a bandeja lê da janela a cada batida.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Retrato {
    /// Se o compartilhamento está parado: sem o outro computador, ou pausado.
    pub parado: bool,
    /// Se o canal de dados está movendo algo agora — cópias e pastas.
    pub atravessando: bool,
    /// Como terminou, ou se anda, a última cópia, se houve alguma.
    pub copia: Option<Tom>,
    /// Se a pessoa já viu o que houve: no Windows, a janela aberta, com o cartão da cópia; no
    /// Linux, onde a janela é outro processo, um clique no ícone ou no menu dele.
    pub janela_visivel: bool,
}

/// A memória do que o ícone já mostrou: o ✓ tem prazo, e o ! espera ser visto.
#[derive(Debug, Default)]
pub struct Selo {
    /// Até quando o ✓ fica.
    feito_ate: Option<Instant>,
    /// Se há uma falha que a pessoa ainda não viu.
    problema: bool,
    /// O estado da cópia na batida anterior, para perceber a mudança.
    copia_antes: Option<Tom>,
}

impl Selo {
    /// Como o ícone fica agora.
    pub fn aparencia(&mut self, retrato: &Retrato, agora: Instant) -> Aparencia {
        if retrato.copia != self.copia_antes {
            match retrato.copia {
                Some(Tom::Feito) => {
                    self.feito_ate = Some(agora + FEITO);
                    self.problema = false;
                }
                Some(Tom::Problema) => {
                    self.feito_ate = None;
                    self.problema = true;
                }
                Some(Tom::Andamento) => {
                    self.feito_ate = None;
                    self.problema = false;
                }
                _ => {}
            }
            self.copia_antes = retrato.copia;
        }
        if retrato.janela_visivel {
            // Com a janela aberta, o cartão da cópia diz o que houve: o ! já foi visto.
            self.problema = false;
        }
        if retrato.copia == Some(Tom::Andamento) || retrato.atravessando {
            Aparencia::Trabalhando
        } else if self.problema {
            Aparencia::Problema
        } else if self.feito_ate.is_some_and(|ate| agora < ate) {
            Aparencia::Feito
        } else if retrato.parado {
            Aparencia::Inativo
        } else {
            Aparencia::Normal
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn retrato(copia: Option<Tom>) -> Retrato {
        Retrato {
            parado: false,
            atravessando: false,
            copia,
            janela_visivel: false,
        }
    }

    #[test]
    fn a_copia_que_anda_gira_e_a_que_chegou_mostra_o_feito_por_um_tempo() {
        let mut selo = Selo::default();
        let t0 = Instant::now();
        assert_eq!(selo.aparencia(&retrato(None), t0), Aparencia::Normal);
        assert_eq!(
            selo.aparencia(&retrato(Some(Tom::Andamento)), t0),
            Aparencia::Trabalhando
        );
        assert_eq!(
            selo.aparencia(&retrato(Some(Tom::Feito)), t0),
            Aparencia::Feito
        );
        assert_eq!(
            selo.aparencia(&retrato(Some(Tom::Feito)), t0 + FEITO),
            Aparencia::Normal,
            "sucesso não pede nada: o ✓ some sozinho"
        );
    }

    #[test]
    fn a_falha_fica_ate_a_janela_abrir() {
        let mut selo = Selo::default();
        let t0 = Instant::now();
        selo.aparencia(&retrato(Some(Tom::Andamento)), t0);
        let longe = t0 + Duration::from_secs(3_600);
        assert_eq!(
            selo.aparencia(&retrato(Some(Tom::Problema)), longe),
            Aparencia::Problema
        );
        let mut aberta = retrato(Some(Tom::Problema));
        aberta.janela_visivel = true;
        assert_eq!(selo.aparencia(&aberta, longe), Aparencia::Normal);
        assert_eq!(
            selo.aparencia(&retrato(Some(Tom::Problema)), longe),
            Aparencia::Normal,
            "visto uma vez, não volta"
        );
    }

    #[test]
    fn copiar_de_novo_apaga_a_falha() {
        let mut selo = Selo::default();
        let t0 = Instant::now();
        selo.aparencia(&retrato(Some(Tom::Problema)), t0);
        selo.aparencia(&retrato(Some(Tom::Andamento)), t0);
        assert_eq!(
            selo.aparencia(&retrato(Some(Tom::Feito)), t0),
            Aparencia::Feito
        );
    }

    #[test]
    fn a_pasta_sincronizando_tambem_gira() {
        let mut selo = Selo::default();
        let mut sincronizando = retrato(None);
        sincronizando.atravessando = true;
        assert_eq!(
            selo.aparencia(&sincronizando, Instant::now()),
            Aparencia::Trabalhando
        );
    }

    #[test]
    fn sem_o_outro_computador_ou_pausado_fica_cinza() {
        let mut selo = Selo::default();
        let mut parado = retrato(None);
        parado.parado = true;
        assert_eq!(selo.aparencia(&parado, Instant::now()), Aparencia::Inativo);
    }
}
