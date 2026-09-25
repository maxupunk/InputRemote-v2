//! O que a tela avisa além do estado da sessão: a pausa, a economia de energia do Wi-Fi, a tela de
//! bloqueio do par e a borda que mudou sozinha.
//!
//! Separado de [`super`] por tamanho: são avisos com frase própria, e não o estado do enlace. E
//! moram aqui, e não na janela, para a escolha de **qual** aviso mostrar ter um lugar só e teste.

use serde::{Deserialize, Serialize};

use super::Estado;
use crate::vocabulario::Borda;

/// O que dizer quando o outro computador está na tela de bloqueio e recusa o que se digita daqui.
///
/// Sem isto o teclado simplesmente parava de funcionar lá, e a pessoa não tinha como saber se era
/// defeito ou proteção.
pub const AVISO_DO_BLOQUEIO_DO_PAR: &str = "O outro computador está na tela de bloqueio, e não \
     aceita o teclado e o mouse daqui ali: o cursor fica deste lado até ele ser desbloqueado. Para \
     desbloqueá-lo daqui, ligue \"Tela de bloqueio: Permitir\" nas Preferências dele.";

/// O aviso que a tela inicial mostra: um só, o que mais importa agora.
///
/// Mostrar três faixas empilhadas é o mesmo que não mostrar nenhuma. A ordem é a de quanto cada
/// uma impede: o que não deixa o produto funcionar vem antes do teclado recusado na tela de
/// bloqueio do outro, que vem antes do mouse que trava em rajadas pela rede.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AvisoPrincipal {
    /// O que impede o produto de funcionar ([`Estado::impedimento`]).
    Impedimento(&'static str),
    /// O outro computador está na tela de bloqueio e recusa o que se digita daqui.
    BloqueioDoPar,
    /// O Wi-Fi de uma das máquinas cochila, com o botão que resolve.
    Rede(AvisoDeRede),
}

impl AvisoPrincipal {
    /// A frase da faixa.
    #[must_use]
    pub const fn frase(&self) -> &'static str {
        match self {
            Self::Impedimento(frase) => frase,
            Self::BloqueioDoPar => AVISO_DO_BLOQUEIO_DO_PAR,
            Self::Rede(aviso) => aviso.frase,
        }
    }

    /// Se a faixa tem o botão "Resolver", e se ele pede ao par (`Some(true)`) ou a esta máquina.
    #[must_use]
    pub const fn resolver_no_par(&self) -> Option<bool> {
        match self {
            Self::Rede(aviso) => Some(aviso.no_par),
            Self::Impedimento(_) | Self::BloqueioDoPar => None,
        }
    }
}

/// O que dizer quando o outro computador mudou de lado na tela dele, e este acompanhou.
///
/// A posição muda sem ninguém tocar nesta tela; sem a frase, parece defeito.
#[must_use]
pub const fn frase_da_borda_ajustada(borda: Borda) -> &'static str {
    match borda {
        Borda::Esquerda => "O outro computador mudou de lado: agora ele fica à esquerda deste.",
        Borda::Direita => "O outro computador mudou de lado: agora ele fica à direita deste.",
        Borda::Acima => "O outro computador mudou de lado: agora ele fica acima deste.",
        Borda::Abaixo => "O outro computador mudou de lado: agora ele fica abaixo deste.",
    }
}

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
    /// O aviso que a tela inicial mostra, se houver algum — o mais importante ([`AvisoPrincipal`]).
    #[must_use]
    pub fn aviso_principal(&self) -> Option<AvisoPrincipal> {
        if let Some(frase) = self.impedimento() {
            return Some(AvisoPrincipal::Impedimento(frase));
        }
        if self.par_recusa_tela_de_bloqueio {
            return Some(AvisoPrincipal::BloqueioDoPar);
        }
        self.aviso_de_rede().map(AvisoPrincipal::Rede)
    }

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
