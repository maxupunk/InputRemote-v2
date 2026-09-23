//! O que a tela avisa além do estado da sessão: a pausa e a economia de energia do Wi-Fi.
//!
//! Separado de [`super`] por tamanho: são avisos com frase própria, e não o estado do enlace.

use serde::{Deserialize, Serialize};

use super::Estado;

/// De que lado o compartilhamento foi pausado.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Pausa {
    /// Aqui: nada atravessa até alguém retomar neste computador.
    Aqui,
    /// No outro computador: ele volta a discar quando for retomado lá.
    NoPar,
}

/// Quando a economia de energia do Wi-Fi de uma máquina está ligada.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EconomiaDoWifi {
    /// Sempre: a placa cochila entre pacotes agora.
    Ligada,
    /// Só na bateria.
    SoNaBateria,
}

/// O aviso de rede que a tela mostra, com o botão que resolve.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AvisoDeRede {
    /// A frase.
    pub frase: &'static str,
    /// Se o botão pede ao par (`true`) ou a esta máquina.
    pub no_par: bool,
}

impl Estado {
    /// O aviso sobre a economia de energia do Wi-Fi, se alguma das duas máquinas estiver cochilando.
    ///
    /// Um aviso só, como o [`Self::impedimento`]. O do par vem primeiro quando os dois estão ligados:
    /// quem olha esta tela é quem sente o mouse travar do outro lado, e a placa que atrasa o que ele
    /// manda é a do computador que recebe.
    #[must_use]
    pub fn aviso_de_rede(&self) -> Option<AvisoDeRede> {
        const PAR: &str = "O Wi-Fi do outro computador está economizando energia: a placa cochila \
                           entre pacotes, e o mouse pela rede trava em rajadas.";
        const AQUI: &str = "O Wi-Fi deste computador está economizando energia: a placa cochila \
                            entre pacotes, e o mouse pela rede trava em rajadas.";
        const PAR_NA_BATERIA: &str = "O Wi-Fi do outro computador economiza energia quando ele \
                                      está na bateria, e aí o mouse pela rede trava em rajadas.";
        const AQUI_NA_BATERIA: &str = "O Wi-Fi deste computador economiza energia na bateria, e \
                                       aí o mouse pela rede trava em rajadas.";
        let aviso = |frase, no_par| Some(AvisoDeRede { frase, no_par });
        match (self.economia_no_par, self.economia_aqui) {
            (Some(EconomiaDoWifi::Ligada), _) => aviso(PAR, true),
            (_, Some(EconomiaDoWifi::Ligada)) => aviso(AQUI, false),
            (Some(EconomiaDoWifi::SoNaBateria), _) => aviso(PAR_NA_BATERIA, true),
            (_, Some(EconomiaDoWifi::SoNaBateria)) => aviso(AQUI_NA_BATERIA, false),
            (None, None) => None,
        }
    }
}
