//! As temperaturas no Windows, só por fonte que não exige driver nosso.
//!
//! Ler o processador e a placa de vídeo com precisão, no Windows, pede um driver de núcleo — o do
//! `LibreHardwareMonitor`, que o Defender bloqueia como vulnerável. O produto não instala driver
//! ([11](../../../../docs/11-nao-legado.md)), então ([12, §2](../../../../docs/12-painel-android.md)):
//!
//! - **se o `LibreHardwareMonitor` (ou o `OpenHardwareMonitor`) estiver rodando**, os sensores dele, pelo
//!   WMI que ele mesmo publica — quem quer mais sensores já o tem;
//! - **a zona térmica do ACPI** (`MSAcpi_ThermalZoneTemperature`), que o serviço lê como `SYSTEM` e
//!   que mede algo perto da placa. Muita máquina não a expõe; aí não há temperatura, e a tela diz
//!   "—".

use std::time::{Duration, Instant};

use ir_proto::message::{Sensor, UsageSample};
use serde::Deserialize;
use wmi::WMIConnection;

use crate::fonte::Fonte;
use crate::leitura::{decimos_de_kelvin_para_centesimos, mais_quentes, sensor_do_lhm};

/// De quanto em quanto tempo procurar de novo um monitor de hardware que não estava rodando.
///
/// O usuário pode abri-lo com o painel já olhando; um minuto de espera é aceitável, e abrir uma
/// conexão WMI a cada leitura não é.
const PROCURAR_DE_NOVO: Duration = Duration::from_secs(60);

/// Os monitores de hardware que publicam os sensores no WMI, com o mesmo esquema.
const MONITORES: [&str; 2] = ["ROOT\\LibreHardwareMonitor", "ROOT\\OpenHardwareMonitor"];

#[derive(Deserialize)]
#[serde(rename = "MSAcpi_ThermalZoneTemperature", rename_all = "PascalCase")]
struct Zona {
    current_temperature: u32,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct SensorDoMonitor {
    identifier: String,
    name: String,
    value: f32,
}

/// As temperaturas que esta máquina deixa ler.
pub(super) struct Termica {
    acpi: Option<WMIConnection>,
    monitor: Option<WMIConnection>,
    procurou_em: Option<Instant>,
}

impl core::fmt::Debug for Termica {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Termica")
            .field("acpi", &self.acpi.is_some())
            .field("monitor", &self.monitor.is_some())
            .finish_non_exhaustive()
    }
}

impl Termica {
    /// Abre a zona térmica do ACPI, se existir. O monitor de hardware é procurado na primeira leitura.
    pub(super) fn nova() -> Self {
        let acpi = WMIConnection::with_namespace_path("ROOT\\WMI")
            .inspect_err(|erro| tracing::debug!(%erro, "sem acesso ao WMI do ACPI"))
            .ok();
        Self {
            acpi,
            monitor: None,
            procurou_em: None,
        }
    }

    fn procurar_monitor(&mut self, agora: Instant) {
        if self.monitor.is_some()
            || self
                .procurou_em
                .is_some_and(|quando| agora.saturating_duration_since(quando) < PROCURAR_DE_NOVO)
        {
            return;
        }
        self.procurou_em = Some(agora);
        self.monitor = MONITORES
            .iter()
            .find_map(|caminho| WMIConnection::with_namespace_path(caminho).ok());
        if self.monitor.is_some() {
            tracing::info!("temperaturas pelo monitor de hardware que está rodando");
        }
    }

    fn do_monitor(&mut self) -> Option<Vec<(Sensor, i16)>> {
        let conexao = self.monitor.as_ref()?;
        let consulta =
            "SELECT Identifier, Name, Value FROM Sensor WHERE SensorType = 'Temperature'";
        let Ok(sensores) = conexao.raw_query::<SensorDoMonitor>(consulta) else {
            // O monitor foi fechado: volta ao ACPI, e procura de novo depois.
            self.monitor = None;
            return None;
        };
        Some(
            sensores
                .into_iter()
                .filter_map(|s| {
                    let sensor = sensor_do_lhm(&s.identifier, &s.name)?;
                    Some((sensor, centesimos(s.value)?))
                })
                .collect(),
        )
    }

    fn do_acpi(&mut self) -> Vec<(Sensor, i16)> {
        let Some(conexao) = self.acpi.as_ref() else {
            return Vec::new();
        };
        let Ok(zonas) = conexao.query::<Zona>() else {
            // Esta máquina não expõe a zona térmica: não adianta perguntar a cada 5 s.
            self.acpi = None;
            return Vec::new();
        };
        zonas
            .into_iter()
            .filter_map(|zona| decimos_de_kelvin_para_centesimos(zona.current_temperature))
            .map(|centi| (Sensor::Board, centi))
            .collect()
    }
}

impl Fonte for Termica {
    fn ler(&mut self, agora: Instant, amostra: &mut UsageSample) {
        self.procurar_monitor(agora);
        let leituras = match self.do_monitor() {
            Some(leituras) if !leituras.is_empty() => leituras,
            _ => self.do_acpi(),
        };
        amostra.temperatures = mais_quentes(leituras);
    }
}

/// Graus em `f32`, do monitor, para centésimos — `None` para o que não é número de sensor.
fn centesimos(graus: f32) -> Option<i16> {
    let centi = (graus * 100.0).round();
    // `i16` cobre de −327 °C a 327 °C; fora disso é sensor desconectado ou leitura inválida.
    if !(-32_000.0..=32_000.0).contains(&centi) {
        return None;
    }
    #[allow(clippy::cast_possible_truncation)]
    Some(centi as i16)
}
