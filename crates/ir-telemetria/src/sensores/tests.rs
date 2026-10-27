use super::*;

fn leitura(sensor: Sensor, nome: &str, centi: i16) -> Leitura {
    (sensor, nome.to_owned(), centi)
}

fn nomes(temperaturas: &[Temperature]) -> Vec<&str> {
    temperaturas.iter().map(|t| t.name.as_str()).collect()
}

#[test]
fn todo_driver_vira_uma_parte_da_maquina() {
    assert_eq!(do_hwmon("k10temp\n"), Sensor::Cpu);
    assert_eq!(do_hwmon("amdgpu"), Sensor::Gpu);
    assert_eq!(do_hwmon("nvme"), Sensor::Disk);
    assert_eq!(do_hwmon("acpitz"), Sensor::Board);
    assert_eq!(do_hwmon("pch_cannonlake"), Sensor::Board);
    assert_eq!(
        do_hwmon("iwlwifi_1"),
        Sensor::Other,
        "a placa de rede também esquenta"
    );
}

#[test]
fn o_nome_do_hwmon_e_o_rotulo_do_driver() {
    assert_eq!(nome_do_hwmon("coretemp", Some("Package id 0\n")), "Package");
    assert_eq!(nome_do_hwmon("coretemp", Some("Core 3")), "Core 3");
    assert_eq!(nome_do_hwmon("nvme", Some("Composite")), "NVMe");
    assert_eq!(nome_do_hwmon("amdgpu", Some("junction")), "junction");
    assert_eq!(nome_do_hwmon("iwlwifi_1", None), "iwlwifi_1");
}

#[test]
fn o_lhm_da_cada_sensor_com_nome_e_deixa_de_fora_o_que_nao_e_temperatura() {
    assert_eq!(
        do_lhm("/intelcpu/0/temperature/8", "CPU Package"),
        Some((Sensor::Cpu, "Package".to_owned()))
    );
    assert_eq!(
        do_lhm("/intelcpu/0/temperature/1", "CPU Core #2"),
        Some((Sensor::Cpu, "Core #2".to_owned()))
    );
    assert_eq!(
        do_lhm("/intelcpu/0/temperature/9", "Core #2 Distance to TjMax"),
        None
    );
    assert_eq!(do_lhm("/intelcpu/0/temperature/10", "Core Average"), None);
    assert_eq!(
        do_lhm("/gpu-nvidia/0/temperature/2", "GPU Hot Spot"),
        Some((Sensor::Gpu, "GPU Hot Spot".to_owned()))
    );
    assert_eq!(
        do_lhm("/nvme/1/temperature/0", "Composite Temperature"),
        Some((Sensor::Disk, "Disco 2".to_owned()))
    );
    assert_eq!(
        do_lhm("/lpc/nct6798d/temperature/1", "CPU"),
        Some((Sensor::Board, "CPU".to_owned()))
    );
}

#[test]
fn a_zona_do_acpi_se_chama_pelo_nome_curto() {
    assert_eq!(nome_do_acpi(r"ACPI\ThermalZone\TZ00_0"), "Zona TZ00");
    assert_eq!(nome_do_acpi("THRM"), "Zona THRM");
}

#[test]
fn em_ordem_de_parte_e_de_numero() {
    let escolhidas = escolher([
        leitura(Sensor::Disk, "NVMe", 3500),
        leitura(Sensor::Cpu, "Core #10", 6000),
        leitura(Sensor::Cpu, "Core #2", 5000),
        leitura(Sensor::Cpu, "Package", 6100),
    ]);
    assert_eq!(
        nomes(&escolhidas),
        vec!["Core #2", "Core #10", "Package", "NVMe"]
    );
}

#[test]
fn com_sensores_demais_cada_parte_tem_vaga() {
    let mut leituras: Vec<Leitura> = (0..20)
        .map(|i| leitura(Sensor::Cpu, &format!("Core #{i}"), 5000 + i))
        .collect();
    leituras.push(leitura(Sensor::Gpu, "GPU Core", 4000));
    leituras.push(leitura(Sensor::Disk, "NVMe", 3000));
    let escolhidas = escolher(leituras);
    assert_eq!(escolhidas.len(), MAX_TEMPERATURES);
    assert!(escolhidas.iter().any(|t| t.sensor == Sensor::Gpu));
    assert!(escolhidas.iter().any(|t| t.sensor == Sensor::Disk));
    assert!(
        escolhidas.iter().any(|t| t.name == "Core #19"),
        "o núcleo mais quente fica"
    );
}

#[test]
fn nomes_repetidos_ganham_numero() {
    let escolhidas = escolher([
        leitura(Sensor::Disk, "NVMe", 3500),
        leitura(Sensor::Disk, "NVMe", 4000),
    ]);
    assert_eq!(nomes(&escolhidas), vec!["NVMe", "NVMe 2"]);
}

/// O fedora de verdade: quatro zonas do ACPI sem rótulo, e um sensor do NVMe no zero absoluto.
#[test]
fn quatro_iguais_viram_quatro_nomes_e_o_zero_absoluto_fica_de_fora() {
    let escolhidas = escolher([
        leitura(Sensor::Board, "Zona ACPI", 5100),
        leitura(Sensor::Board, "Zona ACPI", 4700),
        leitura(Sensor::Board, "Zona ACPI", 4500),
        leitura(Sensor::Board, "Zona ACPI", 4300),
        leitura(Sensor::Disk, "Sensor 1", -27315),
    ]);
    assert_eq!(
        nomes(&escolhidas),
        vec!["Zona ACPI", "Zona ACPI 2", "Zona ACPI 3", "Zona ACPI 4"]
    );
    assert_eq!(nome_do_hwmon("acpitz", None), "Zona ACPI");
}
