//! A amostra de uso de uma máquina: o que o painel no Android mostra
//! ([12](../../../../docs/12-painel-android.md)).
//!
//! Viaja em dois lugares, com a mesma forma: de um computador ao par, dentro do recado do painel
//! ([`Control::Panel`](super::Control::Panel), desde a versão 11), e do computador que agrega ao
//! painel, pelo enlace do painel (`ir-monitor`). Por isso mora aqui, com vetores gravados.
//!
//! Tudo é inteiro e em ponto fixo: a amostra sai uma vez por segundo, e o `postcard` codifica
//! inteiro pequeno em poucos bytes. Um campo `None` é "esta máquina não sabe ler isto", e a tela
//! mostra "—": não é zero, e não é erro.

use serde::{Deserialize, Serialize};

use crate::limits;

/// Uma amostra de uso, tirada de uma vez.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct UsageSample {
    /// Uso total do processador, em milésimos (`0..=1000`).
    pub cpu_permille: Option<u16>,
    /// A memória principal.
    pub memory: Option<Occupancy>,
    /// O disco do sistema.
    pub disk: Option<Occupancy>,
    /// O tráfego de rede somado das interfaces físicas.
    pub network: Option<Traffic>,
    /// A bateria, quando a máquina tem uma.
    pub battery: Option<Battery>,
    /// As temperaturas que esta máquina consegue ler, no máximo
    /// [`limits::MAX_TEMPERATURES`].
    pub temperatures: Vec<Temperature>,
}

impl UsageSample {
    /// A amostra dentro dos tetos do protocolo: o processador em `0..=1000`, a ocupação nunca
    /// maior que o total, e no máximo [`limits::MAX_TEMPERATURES`] temperaturas.
    ///
    /// Quem recebe chama isto antes de mostrar: a amostra vem de outra máquina, e um número
    /// impossível na tela é pior que um "—".
    #[must_use]
    pub fn bounded(mut self) -> Self {
        let within = |o: Occupancy| Occupancy {
            used_kib: o.used_kib.min(o.total_kib),
            ..o
        };
        self.cpu_permille = self.cpu_permille.map(|value| value.min(1000));
        self.memory = self.memory.map(within);
        self.disk = self.disk.map(within);
        if let Some(battery) = self.battery.as_mut() {
            battery.percent = battery.percent.min(100);
        }
        self.temperatures.truncate(limits::MAX_TEMPERATURES);
        self
    }
}

/// Quanto de um recurso está em uso, de quanto há.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Occupancy {
    /// Em uso, em KiB.
    pub used_kib: u64,
    /// O total, em KiB.
    pub total_kib: u64,
}

/// Bytes por segundo, nos dois sentidos.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Traffic {
    /// Recebidos por segundo.
    pub received_per_s: u64,
    /// Enviados por segundo.
    pub sent_per_s: u64,
}

/// A carga da bateria e se a máquina está na tomada.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Battery {
    /// A carga, de 0 a 100.
    pub percent: u8,
    /// Se está na tomada.
    pub plugged: bool,
}

/// Uma temperatura, e de onde ela vem.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Temperature {
    /// Que parte da máquina.
    pub sensor: Sensor,
    /// Em centésimos de grau Celsius: `6150` é 61,5 °C. `i16` vai de −327 °C a 327 °C, que cobre
    /// qualquer sensor real.
    pub centi_celsius: i16,
}

/// Que parte da máquina uma temperatura mede.
///
/// A ordem das variantes **é o formato de fio**. Acrescentar no fim é seguro.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Sensor {
    /// O processador (pacote, ou o núcleo mais quente).
    Cpu,
    /// A placa de vídeo.
    Gpu,
    /// Um disco.
    Disk,
    /// A placa-mãe, ou a zona térmica do ACPI, que mede algo perto dela.
    Board,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uma_amostra_impossivel_volta_dentro_dos_tetos() {
        let sample = UsageSample {
            cpu_permille: Some(4000),
            memory: Some(Occupancy {
                used_kib: 9,
                total_kib: 4,
            }),
            disk: None,
            network: None,
            battery: Some(Battery {
                percent: 250,
                plugged: true,
            }),
            temperatures: vec![
                Temperature {
                    sensor: Sensor::Cpu,
                    centi_celsius: 5000,
                };
                limits::MAX_TEMPERATURES + 3
            ],
        }
        .bounded();
        assert_eq!(sample.cpu_permille, Some(1000));
        assert_eq!(sample.memory.map(|m| m.used_kib), Some(4));
        assert_eq!(sample.battery.map(|b| b.percent), Some(100));
        assert_eq!(sample.temperatures.len(), limits::MAX_TEMPERATURES);
    }
}
