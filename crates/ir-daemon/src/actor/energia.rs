//! A economia de energia do Wi-Fi: verificar a placa daqui, contar ao par, e desligar quando pedem.
//!
//! Com a economia ligada, a placa cochila entre pacotes e o mouse pela rede trava em rajadas — o que
//! o Bluetooth não sofre, e o Deskflow sofre igual ([log 44](../../../../docs/logs/44-o-wifi-que-cochilava.md)).
//! Quem sente é quem olha a tela do **outro** lado, então o estado daqui vai ao par pela sessão, e o
//! botão da janela pode pedir que qualquer uma das duas placas pare de cochilar.
//!
//! As verificações e a correção rodam comandos do sistema, que bloqueiam: vão para uma thread de
//! bloqueio, e o resultado volta ao ator como [`DeFundo::Economia`](super::DeFundo).

use ir_energia::Economia;
use ir_ipc::{Aviso, EconomiaDoWifi, Falha, Resposta};
use ir_proto::message::NetworkPowerSaving;
use ir_session::Input;
use tracing::{info, warn};

use super::Daemon;

impl Daemon {
    /// Verifica a placa daqui, fora do ator. Sem runtime (os testes síncronos) não há verificação.
    pub(crate) fn verificar_economia(&self) {
        let Ok(runtime) = tokio::runtime::Handle::try_current() else {
            return;
        };
        let de_fundo = self.de_fundo.clone();
        runtime.spawn_blocking(move || {
            let _ = de_fundo.send(super::DeFundo::Economia(ir_energia::verificar()));
        });
    }

    /// Desliga a economia da placa daqui, fora do ator, e verifica de novo em seguida.
    pub(crate) fn desligar_economia_aqui(&self) {
        let Ok(runtime) = tokio::runtime::Handle::try_current() else {
            return;
        };
        let de_fundo = self.de_fundo.clone();
        runtime.spawn_blocking(move || {
            match ir_energia::desligar() {
                Ok(()) => info!("economia de energia do Wi-Fi desligada"),
                Err(erro) => warn!(%erro, "não foi possível desligar a economia do Wi-Fi"),
            }
            let _ = de_fundo.send(super::DeFundo::Economia(ir_energia::verificar()));
        });
    }

    /// Chegou uma verificação da placa daqui.
    ///
    /// Toda verificação vai à sessão, que a repete ao par: se uma mudança não chegou lá, a próxima
    /// verificação a leva. O registro e a janela só mudam quando a placa muda.
    pub(crate) fn on_economia(&mut self, economia: Economia) {
        // Não saber não apaga o que se sabia: uma ferramenta que falhou uma vez não é placa nova.
        if economia == Economia::Desconhecida {
            return;
        }
        let mudou = economia != self.economia_aqui;
        self.economia_aqui = economia;
        let no_protocolo = match economia {
            Economia::Ligada => NetworkPowerSaving::On,
            Economia::SoNaBateria => NetworkPowerSaving::OnBattery,
            Economia::Desligada | Economia::Desconhecida => NetworkPowerSaving::Off,
        };
        self.drive(Input::LocalNetworkPower(no_protocolo));
        if mudou {
            info!(?economia, "economia de energia do Wi-Fi desta máquina");
            let _ = self.avisos.send(Aviso::EstadoMudou(self.estado()));
        }
    }

    /// O par contou como está a placa dele.
    pub(crate) fn on_economia_do_par(&mut self, estado: Option<NetworkPowerSaving>) {
        if estado == self.economia_no_par {
            return; // a repetição periódica do par não é notícia
        }
        if let Some(estado) = estado {
            info!(?estado, "economia de energia do Wi-Fi do par");
        }
        self.economia_no_par = estado;
        let _ = self.avisos.send(Aviso::EstadoMudou(self.estado()));
    }

    /// O botão do aviso: desligar a economia daqui, ou pedir ao par.
    pub(crate) fn desligar_economia(&mut self, no_par: bool) -> Resposta {
        if !no_par {
            self.desligar_economia_aqui();
            return Resposta::Feito;
        }
        let entende = self.session.phase().is_established()
            && self
                .session
                .peer()
                .is_some_and(|par| par.version >= ir_proto::version::NETWORK_POWER);
        if !entende {
            return Resposta::Falha(Falha::ForaDeContexto);
        }
        self.drive(Input::DisablePeerNetworkPowerSaving);
        Resposta::Feito
    }

    /// A economia daqui, no vocabulário da janela — só quando atrapalha.
    pub(crate) const fn economia_aqui_na_tela(&self) -> Option<EconomiaDoWifi> {
        match self.economia_aqui {
            Economia::Ligada => Some(EconomiaDoWifi::Ligada),
            Economia::SoNaBateria => Some(EconomiaDoWifi::SoNaBateria),
            Economia::Desligada | Economia::Desconhecida => None,
        }
    }

    /// A economia do par, no vocabulário da janela — só quando atrapalha.
    pub(crate) const fn economia_no_par_na_tela(&self) -> Option<EconomiaDoWifi> {
        match self.economia_no_par {
            Some(NetworkPowerSaving::On) => Some(EconomiaDoWifi::Ligada),
            Some(NetworkPowerSaving::OnBattery) => Some(EconomiaDoWifi::SoNaBateria),
            Some(NetworkPowerSaving::Off) | None => None,
        }
    }
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod testes;
