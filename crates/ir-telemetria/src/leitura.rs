//! O que o sistema escreve, virando número. Puro: recebe texto, devolve valor.
//!
//! Mora fora dos módulos de cada sistema para os testes rodarem em qualquer máquina — o formato
//! de `/proc/stat` não muda porque o teste roda no Windows.

use ir_proto::message::{Sensor, Temperature};

use crate::conta::TemposDoProcessador;

/// A primeira linha de `/proc/stat`: `cpu  user nice system idle iowait irq softirq steal …`.
///
/// Ocioso é `idle + iowait`: esperar o disco não é trabalho do processador. O total é a soma das
/// oito primeiras colunas; `guest` já está contado em `user` e ficaria em dobro.
#[must_use]
pub fn proc_stat(texto: &str) -> Option<TemposDoProcessador> {
    let linha = texto.lines().find(|linha| linha.starts_with("cpu "))?;
    let colunas: Vec<u64> = linha
        .split_whitespace()
        .skip(1)
        .take(8)
        .map(str::parse)
        .collect::<Result<_, _>>()
        .ok()?;
    let ocioso = colunas.get(3)? + colunas.get(4).copied().unwrap_or(0);
    Some(TemposDoProcessador {
        ocioso,
        total: colunas.iter().sum(),
    })
}

/// `MemTotal` e `MemAvailable` de `/proc/meminfo`, em KiB: (usado, total).
///
/// Usado é o total menos o disponível — o que o kernel conta como reaproveitável (cache,
/// *buffers*) não é memória ocupada para quem olha o painel.
#[must_use]
pub fn proc_meminfo(texto: &str) -> Option<(u64, u64)> {
    let campo = |nome: &str| {
        texto
            .lines()
            .find_map(|linha| linha.strip_prefix(nome))
            .and_then(|resto| resto.split_whitespace().next())
            .and_then(|numero| numero.parse::<u64>().ok())
    };
    let total = campo("MemTotal:")?;
    let disponivel = campo("MemAvailable:")?;
    Some((total.saturating_sub(disponivel), total))
}

/// Os bytes recebidos e enviados, somados, das interfaces físicas de `/proc/net/dev`.
///
/// Fica de fora o que não é tráfego da máquina com o mundo: o *loopback* e as pontes e pares
/// virtuais de contêineres e máquinas virtuais, que contariam o mesmo pacote duas vezes.
#[must_use]
pub fn proc_net_dev(texto: &str) -> Option<(u64, u64)> {
    let mut soma = None;
    for linha in texto.lines().skip(2) {
        let Some((nome, numeros)) = linha.split_once(':') else {
            continue;
        };
        if interface_virtual(nome.trim()) {
            continue;
        }
        let colunas: Vec<u64> = numeros
            .split_whitespace()
            .filter_map(|n| n.parse().ok())
            .collect();
        let (Some(recebidos), Some(enviados)) = (colunas.first(), colunas.get(8)) else {
            continue;
        };
        let (r, e) = soma.unwrap_or((0u64, 0u64));
        soma = Some((r.saturating_add(*recebidos), e.saturating_add(*enviados)));
    }
    soma
}

/// Um `/proc/<pid>/stat`: o nome, o tempo de processador (`utime + stime`, em *ticks*, a mesma
/// unidade de `/proc/stat`) e a memória residente em páginas.
///
/// O nome vem entre parênteses e pode ter espaço e parêntese dentro (`(Web Content)`): os números
/// começam depois do **último** `)`.
#[must_use]
pub fn proc_pid_stat(texto: &str) -> Option<(&str, u64, u64)> {
    let abre = texto.find('(')?;
    let fecha = texto.rfind(')')?;
    let nome = texto.get(abre + 1..fecha)?;
    // Depois do nome, o campo 3 do `man 5 proc` é o primeiro: `utime` é o 14, `stime` o 15 e
    // `rss` o 24.
    let campos: Vec<&str> = texto.get(fecha + 1..)?.split_whitespace().collect();
    let numero = |campo: usize| campos.get(campo - 3)?.parse::<u64>().ok();
    let tempo = numero(14)?.saturating_add(numero(15)?);
    Some((nome, tempo, numero(24)?))
}

fn interface_virtual(nome: &str) -> bool {
    const PREFIXOS: [&str; 8] = [
        "lo",
        "docker",
        "veth",
        "br-",
        "virbr",
        "vnet",
        "tun",
        "tailscale",
    ];
    PREFIXOS.iter().any(|prefixo| nome.starts_with(prefixo))
}

/// Que parte da máquina um `hwmon` mede, pelo nome do driver (`/sys/class/hwmon/*/name`).
///
/// Só os drivers conhecidos entram: um sensor que não se sabe o que mede é um número que engana.
#[must_use]
pub fn sensor_do_hwmon(nome: &str) -> Option<Sensor> {
    match nome.trim() {
        "coretemp" | "k10temp" | "zenpower" | "cpu_thermal" => Some(Sensor::Cpu),
        "amdgpu" | "radeon" | "nouveau" | "i915" | "xe" => Some(Sensor::Gpu),
        "nvme" | "drivetemp" => Some(Sensor::Disk),
        "acpitz" => Some(Sensor::Board),
        _ => None,
    }
}

/// Um `tempN_input` do `hwmon`, em milésimos de grau, para centésimos.
#[must_use]
pub fn milesimos_para_centesimos(texto: &str) -> Option<i16> {
    let milesimos: i32 = texto.trim().parse().ok()?;
    i16::try_from(milesimos / 10).ok()
}

/// A bateria de `/sys/class/power_supply/BAT*`: `capacity` e `status`.
///
/// Na tomada é qualquer estado que não seja "descarregando": carregando, cheia, ou parada na
/// tomada (`Not charging`, a carga limitada que muitos notebooks usam).
#[must_use]
pub fn bateria(capacidade: &str, estado: &str) -> Option<(u8, bool)> {
    let carga: u8 = capacidade.trim().parse().ok()?;
    Some((carga.min(100), estado.trim() != "Discharging"))
}

/// A zona térmica do ACPI no Windows: `CurrentTemperature` vem em décimos de kelvin.
#[must_use]
pub fn decimos_de_kelvin_para_centesimos(decimos: u32) -> Option<i16> {
    let centesimos = i64::from(decimos) * 10 - 27_315;
    i16::try_from(centesimos).ok()
}

/// Que parte da máquina um sensor do `LibreHardwareMonitor` mede, pelo `Identifier` dele —
/// `/amdcpu/0/temperature/2`, `/gpu-nvidia/0/temperature/0`, `/nvme/1/temperature/0`.
///
/// Do processador vale só o pacote (`Package`, `Tctl`, `Core Max`): os núcleos um a um encheriam o
/// painel de números que dizem a mesma coisa.
#[must_use]
pub fn sensor_do_lhm(identificador: &str, nome: &str) -> Option<Sensor> {
    let hardware = identificador.trim_start_matches('/').split('/').next()?;
    if hardware.ends_with("cpu") {
        let pacote = ["Package", "Tctl", "Core Max", "CPU Total"];
        return pacote
            .iter()
            .any(|p| nome.contains(p))
            .then_some(Sensor::Cpu);
    }
    if hardware.starts_with("gpu") {
        return (nome.contains("Core") || nome == "GPU").then_some(Sensor::Gpu);
    }
    if matches!(hardware, "nvme" | "hdd" | "ssd") {
        return Some(Sensor::Disk);
    }
    hardware.starts_with("lpc").then_some(Sensor::Board)
}

/// Uma temperatura por parte da máquina — a mais quente —, na ordem de [`Sensor`].
///
/// Dois discos ou duas zonas do ACPI dizem a mesma coisa ao painel: o que importa é o mais quente.
/// O resultado cabe em [`MAX_TEMPERATURES`](ir_proto::limits::MAX_TEMPERATURES) por construção.
#[must_use]
pub fn mais_quentes(leituras: impl IntoIterator<Item = (Sensor, i16)>) -> Vec<Temperature> {
    let mut por_sensor: Vec<Temperature> = Vec::new();
    for (sensor, centi_celsius) in leituras {
        match por_sensor.iter_mut().find(|t| t.sensor == sensor) {
            Some(atual) => atual.centi_celsius = atual.centi_celsius.max(centi_celsius),
            None => por_sensor.push(Temperature {
                sensor,
                centi_celsius,
            }),
        }
    }
    por_sensor.sort_by_key(|t| t.sensor);
    por_sensor
}

#[cfg(test)]
mod tests;
