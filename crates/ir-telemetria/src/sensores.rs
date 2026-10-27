//! Os sensores de temperatura: de que parte da máquina é cada um, como ele se chama no card, e
//! quais entram na amostra. Puro, para os testes rodarem em qualquer sistema — as fontes de cada
//! sistema só leem e chamam isto.

use std::collections::BTreeMap;

use ir_monitor::amostra::MAX_TEMPERATURES;
use ir_monitor::amostra::{Sensor, Temperature};

/// Uma leitura crua: a parte da máquina, o nome do sensor e os centésimos de grau.
pub type Leitura = (Sensor, String, i16);

/// Que parte da máquina um `hwmon` mede, pelo nome do driver (`/sys/class/hwmon/*/name`). Um
/// driver que o painel não conhece é [`Sensor::Other`]: a placa de rede, a bateria — ainda é uma
/// temperatura desta máquina, e a pessoa pode querer o card dela.
#[must_use]
pub fn do_hwmon(driver: &str) -> Sensor {
    match driver.trim() {
        "coretemp" | "k10temp" | "zenpower" | "cpu_thermal" => Sensor::Cpu,
        "amdgpu" | "radeon" | "nouveau" | "i915" | "xe" => Sensor::Gpu,
        "nvme" | "drivetemp" => Sensor::Disk,
        "acpitz" => Sensor::Board,
        nome if nome.starts_with("pch_") => Sensor::Board,
        _ => Sensor::Other,
    }
}

/// O nome de um sensor do `hwmon`: o rótulo que o driver dá (`temp1_label`: "Package id 0",
/// "Composite", "edge"), ou o próprio driver quando ele não dá.
#[must_use]
pub fn nome_do_hwmon(driver: &str, rotulo: Option<&str>) -> String {
    let rotulo = rotulo.map(str::trim).filter(|r| !r.is_empty());
    let driver = driver.trim();
    match (do_hwmon(driver), rotulo) {
        (Sensor::Disk, Some("Composite") | None) => "NVMe".to_owned(),
        (_, Some(rotulo)) => rotulo.replace("Package id 0", "Package"),
        (_, None) => driver.to_owned(),
    }
}

/// Que parte da máquina um sensor do `LibreHardwareMonitor` mede, e o nome dele no card — pelo
/// `Identifier` (`/amdcpu/0/temperature/2`, `/gpu-nvidia/0/temperature/0`, `/nvme/1/temperature/0`)
/// e pelo `Name`.
///
/// Fica de fora o que não é temperatura de uma parte: `Distance to TjMax` é quanto falta, e a
/// média dos núcleos repete o que os núcleos dizem.
#[must_use]
pub fn do_lhm(identificador: &str, nome: &str) -> Option<(Sensor, String)> {
    if nome.contains("Distance") || nome.contains("Average") {
        return None;
    }
    let mut partes = identificador.trim_start_matches('/').split('/');
    let hardware = partes.next()?;
    let indice = partes
        .next()
        .and_then(|i| i.parse::<u32>().ok())
        .unwrap_or(0);
    let sensor = if hardware.ends_with("cpu") {
        Sensor::Cpu
    } else if hardware.starts_with("gpu") {
        Sensor::Gpu
    } else if matches!(hardware, "nvme" | "hdd" | "ssd") {
        Sensor::Disk
    } else if hardware.starts_with("lpc") {
        Sensor::Board
    } else {
        Sensor::Other
    };
    let nome = match sensor {
        // "Temperature", "Composite Temperature": o disco se diz pelo número, e não pelo sensor.
        Sensor::Disk => format!("Disco {}", indice + 1),
        Sensor::Cpu => nome.trim_start_matches("CPU ").to_owned(),
        _ => nome.to_owned(),
    };
    Some((sensor, nome))
}

/// O nome de uma zona térmica do ACPI pelo `InstanceName` (`ACPI\ThermalZone\TZ00_0`): "TZ00".
#[must_use]
pub fn nome_do_acpi(instancia: &str) -> String {
    let zona = instancia.rsplit('\\').next().unwrap_or(instancia);
    let zona = zona.rsplit_once('_').map_or(zona, |(antes, _)| antes);
    format!("Zona {zona}")
}

/// As leituras que entram na amostra, em ordem de parte e de nome, com nomes únicos.
///
/// Quando há mais sensores que [`MAX_TEMPERATURES`], a escolha é **justa**: uma vaga para o mais
/// quente de cada parte, depois o segundo de cada uma, e assim por diante. Um processador de
/// dezesseis núcleos não esconde a placa de vídeo e os discos.
#[must_use]
pub fn escolher(leituras: impl IntoIterator<Item = Leitura>) -> Vec<Temperature> {
    let mut por_parte: BTreeMap<Sensor, Vec<(String, i16)>> = BTreeMap::new();
    for (sensor, nome, centi) in leituras {
        por_parte.entry(sensor).or_default().push((nome, centi));
    }
    for grupo in por_parte.values_mut() {
        grupo.sort_by_key(|(_, centi)| core::cmp::Reverse(*centi));
    }
    let mut escolhidas: Vec<Temperature> = Vec::new();
    let mut rodada = 0;
    while escolhidas.len() < MAX_TEMPERATURES {
        let desta: Vec<Temperature> = por_parte
            .iter()
            .filter_map(|(sensor, grupo)| {
                let (nome, centi) = grupo.get(rodada)?;
                Some(Temperature {
                    sensor: *sensor,
                    centi_celsius: *centi,
                    name: nome.clone(),
                })
            })
            .collect();
        if desta.is_empty() {
            break;
        }
        escolhidas.extend(desta.into_iter().take(MAX_TEMPERATURES - escolhidas.len()));
        rodada += 1;
    }
    escolhidas.sort_by(|a, b| {
        a.sensor
            .cmp(&b.sensor)
            .then_with(|| natural(&a.name, &b.name))
    });
    unicos(&mut escolhidas);
    escolhidas
}

/// "Core #2" antes de "Core #10": o número dentro do nome comparado como número.
fn natural(a: &str, b: &str) -> core::cmp::Ordering {
    let chave = |texto: &str| {
        let corte = texto
            .find(|c: char| c.is_ascii_digit())
            .unwrap_or(texto.len());
        let (prefixo, resto) = texto.split_at(corte);
        let numero: String = resto.chars().take_while(char::is_ascii_digit).collect();
        (
            prefixo.to_owned(),
            numero.parse::<u64>().unwrap_or(0),
            texto.to_owned(),
        )
    };
    chave(a).cmp(&chave(b))
}

/// Dois sensores com o mesmo nome na mesma parte (dois discos `NVMe`) viram `NVMe` e `NVMe 2`: o
/// nome é a chave do card, e dois cards com a mesma chave seriam um só.
fn unicos(temperaturas: &mut [Temperature]) {
    for i in 1..temperaturas.len() {
        let (antes, depois) = temperaturas.split_at_mut(i);
        let Some(atual) = depois.first_mut() else {
            continue;
        };
        let repetidas = antes
            .iter()
            .filter(|t| t.sensor == atual.sensor && t.name == atual.name)
            .count();
        if repetidas > 0 {
            atual.name = format!("{} {}", atual.name, repetidas + 1);
        }
    }
}

#[cfg(test)]
mod tests;
