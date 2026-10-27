//! A amostra de uso de uma máquina: o que o painel no Android mostra
//! ([12](../../../../docs/12-painel-android.md)).
//!
//! Viaja em dois lugares, com a mesma forma: de um computador ao par, dentro do recado do painel
//! (`Control::Panel`, opaco para a sessão), e do computador que agrega ao painel, pelo enlace do
//! painel. Os dois são vocabulário deste crate, e os vetores gravados estão em `tests/vetores.rs`.
//!
//! Morava no `ir-proto` enquanto viajava em mensagens dele (`Control::Usage`); desde que a versão
//! 11 virou o recado opaco, nenhuma mensagem do protocolo a usa
//! ([log 72](../../../docs/logs/72-gpu-e-todos-os-sensores.md)).
//!
//! Tudo é inteiro e em ponto fixo: a amostra sai uma vez por segundo, e o `postcard` codifica
//! inteiro pequeno em poucos bytes. Um campo `None` é "esta máquina não sabe ler isto", e a tela
//! mostra "—": não é zero, e não é erro.

use serde::{Deserialize, Serialize};

/// Máximo de temperaturas numa amostra de uso.
///
/// Origem: o painel mostra um card por sensor ([log 72](../../../docs/logs/72-gpu-e-todos-os-sensores.md)).
/// Doze cobre o pacote e os núcleos de um processador comum, a GPU, dois discos e a placa; com
/// nomes de até [`MAX_SENSOR_NAME`], são uns 240 bytes, e a amostra cabe no recado do rádio
/// ([`MAX_PANEL_MESSAGE`](ir_proto::MAX_PANEL_MESSAGE)) com folga para os processos.
pub const MAX_TEMPERATURES: usize = 12;

/// Máximo de bytes no nome de um sensor de temperatura ("CPU Package", "GPU Hot Spot").
pub const MAX_SENSOR_NAME: usize = 16;

/// Máximo de placas de vídeo numa amostra: a integrada e a dedicada de um notebook.
pub const MAX_GPUS: usize = 2;

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
    /// As temperaturas que esta máquina consegue ler, uma por sensor, no máximo
    /// [`MAX_TEMPERATURES`].
    pub temperatures: Vec<Temperature>,
    /// As placas de vídeo, no máximo [`MAX_GPUS`]; a de mais memória primeiro.
    pub gpus: Vec<Gpu>,
}

impl UsageSample {
    /// A amostra dentro dos tetos do protocolo: o processador e a GPU em `0..=1000`, a ocupação
    /// nunca maior que o total, no máximo [`MAX_TEMPERATURES`] temperaturas com nomes de
    /// até [`MAX_SENSOR_NAME`] bytes, e no máximo [`MAX_GPUS`] placas.
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
        self.temperatures.truncate(MAX_TEMPERATURES);
        for temperatura in &mut self.temperatures {
            let tamanho = cortar(&temperatura.name, MAX_SENSOR_NAME).len();
            temperatura.name.truncate(tamanho);
        }
        self.gpus.truncate(MAX_GPUS);
        for gpu in &mut self.gpus {
            gpu.busy_permille = gpu.busy_permille.map(|value| value.min(1000));
            gpu.memory = gpu.memory.map(within);
        }
        self
    }
}

/// O maior prefixo de `texto` com até `maximo` bytes, sem partir um caractere.
fn cortar(texto: &str, maximo: usize) -> &str {
    let mut fim = texto.len().min(maximo);
    while !texto.is_char_boundary(fim) {
        fim -= 1;
    }
    texto.get(..fim).unwrap_or("")
}

/// Uma placa de vídeo.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Gpu {
    /// Quem fez: dá o nome do card quando há duas.
    pub vendor: GpuVendor,
    /// O uso, em milésimos: o motor mais ocupado, como o Gerenciador de Tarefas mostra. `None` onde
    /// o sistema não conta (a Intel e a NVIDIA no Linux, sem ferramenta deles).
    pub busy_permille: Option<u16>,
    /// A memória dedicada; `None` numa integrada, que usa a principal.
    pub memory: Option<Occupancy>,
}

/// Quem fez a placa de vídeo. A ordem das variantes **é o formato de fio**.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum GpuVendor {
    /// Outra, ou não se sabe.
    Other,
    /// NVIDIA.
    Nvidia,
    /// AMD.
    Amd,
    /// Intel.
    Intel,
}

impl GpuVendor {
    /// Pelo identificador PCI do fabricante.
    #[must_use]
    pub const fn from_pci(id: u32) -> Self {
        match id {
            0x10DE => Self::Nvidia,
            0x1002 | 0x1022 => Self::Amd,
            0x8086 => Self::Intel,
            _ => Self::Other,
        }
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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Temperature {
    /// Que parte da máquina.
    pub sensor: Sensor,
    /// Em centésimos de grau Celsius: `6150` é 61,5 °C. `i16` vai de −327 °C a 327 °C, que cobre
    /// qualquer sensor real.
    pub centi_celsius: i16,
    /// O nome do sensor, curto ("Package", "GPU Hot Spot", "Disco 2"): o card de cada um.
    pub name: String,
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
    /// Outra parte: a placa de rede, a bateria, um chip que o sistema conhece e o painel não.
    Other,
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
                    name: "ç".repeat(MAX_SENSOR_NAME),
                };
                MAX_TEMPERATURES + 3
            ],
            gpus: vec![
                Gpu {
                    vendor: GpuVendor::Nvidia,
                    busy_permille: Some(5000),
                    memory: None,
                };
                MAX_GPUS + 1
            ],
        }
        .bounded();
        assert_eq!(sample.cpu_permille, Some(1000));
        assert_eq!(sample.memory.map(|m| m.used_kib), Some(4));
        assert_eq!(sample.battery.map(|b| b.percent), Some(100));
        assert_eq!(sample.temperatures.len(), MAX_TEMPERATURES);
        assert!(sample.temperatures[0].name.len() <= MAX_SENSOR_NAME);
        assert_eq!(sample.gpus.len(), MAX_GPUS);
        assert_eq!(sample.gpus[0].busy_permille, Some(1000));
    }
}
