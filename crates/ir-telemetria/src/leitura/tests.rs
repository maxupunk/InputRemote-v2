use super::*;

const STAT: &str = "cpu  4705 356 584 3699 23 23 0 0 0 0\n\
                    cpu0 1393280 32966 572056 13343292 6130 0 17875 0 23933 0\n";

#[test]
fn proc_stat_soma_as_oito_colunas_e_conta_iowait_como_ocioso() {
    let tempos = proc_stat(STAT).unwrap();
    assert_eq!(tempos.ocioso, 3699 + 23);
    assert_eq!(tempos.total, 4705 + 356 + 584 + 3699 + 23 + 23);
}

#[test]
fn proc_stat_sem_a_linha_total_nao_le() {
    assert!(proc_stat("cpu0 1 2 3 4\n").is_none());
    assert!(proc_stat("cpu  x y z\n").is_none());
}

#[test]
fn meminfo_usa_o_disponivel_e_nao_o_livre() {
    let texto = "MemTotal:       16314380 kB\n\
                 MemFree:          512000 kB\n\
                 MemAvailable:   11000000 kB\n";
    assert_eq!(proc_meminfo(texto), Some((5_314_380, 16_314_380)));
    assert_eq!(proc_meminfo("MemTotal: 10 kB\n"), None);
}

#[test]
fn net_dev_soma_as_fisicas_e_ignora_as_virtuais() {
    let texto = "Inter-|   Receive                            |  Transmit\n \
         face |bytes    packets errs drop fifo frame compressed multicast|bytes    packets\n    \
         lo: 1000 10 0 0 0 0 0 0 1000 10 0 0 0 0 0 0\n  \
         eth0: 500 5 0 0 0 0 0 0 300 3 0 0 0 0 0 0\n\
         wlp2s0: 70 1 0 0 0 0 0 0 20 1 0 0 0 0 0 0\n\
         docker0: 9999 1 0 0 0 0 0 0 9999 1 0 0 0 0 0 0\n";
    assert_eq!(proc_net_dev(texto), Some((570, 320)));
}

#[test]
fn net_dev_sem_interface_fisica_nao_tem_trafego() {
    let texto = "a\nb\n    lo: 1 1 0 0 0 0 0 0 1 1 0 0 0 0 0 0\n";
    assert_eq!(proc_net_dev(texto), None);
}

#[test]
fn hwmon_conhecido_vira_sensor_e_desconhecido_fica_de_fora() {
    assert_eq!(sensor_do_hwmon("k10temp\n"), Some(Sensor::Cpu));
    assert_eq!(sensor_do_hwmon("amdgpu"), Some(Sensor::Gpu));
    assert_eq!(sensor_do_hwmon("nvme"), Some(Sensor::Disk));
    assert_eq!(sensor_do_hwmon("acpitz"), Some(Sensor::Board));
    assert_eq!(sensor_do_hwmon("iwlwifi_1"), None);
}

#[test]
fn temperatura_em_milesimos_vira_centesimos() {
    assert_eq!(milesimos_para_centesimos("61500\n"), Some(6150));
    assert_eq!(milesimos_para_centesimos("lixo"), None);
}

#[test]
fn bateria_parada_na_tomada_conta_como_na_tomada() {
    assert_eq!(bateria("76\n", "Discharging\n"), Some((76, false)));
    assert_eq!(bateria("80", "Not charging"), Some((80, true)));
    assert_eq!(bateria("100", "Full"), Some((100, true)));
    assert_eq!(bateria("", "Full"), None);
}

#[test]
fn kelvin_do_acpi_vira_celsius() {
    // 3132 décimos de kelvin = 313,2 K = 40,05 °C.
    assert_eq!(decimos_de_kelvin_para_centesimos(3132), Some(4005));
}

#[test]
fn lhm_pega_o_pacote_do_processador_e_ignora_os_nucleos() {
    assert_eq!(
        sensor_do_lhm("/amdcpu/0/temperature/2", "Core (Tctl/Tdie)"),
        Some(Sensor::Cpu)
    );
    assert_eq!(
        sensor_do_lhm("/intelcpu/0/temperature/8", "CPU Package"),
        Some(Sensor::Cpu)
    );
    assert_eq!(
        sensor_do_lhm("/intelcpu/0/temperature/1", "CPU Core #2"),
        None
    );
    assert_eq!(
        sensor_do_lhm("/gpu-nvidia/0/temperature/0", "GPU Core"),
        Some(Sensor::Gpu)
    );
    assert_eq!(
        sensor_do_lhm("/gpu-nvidia/0/temperature/2", "GPU Hot Spot"),
        None
    );
    assert_eq!(
        sensor_do_lhm("/nvme/1/temperature/0", "Temperature"),
        Some(Sensor::Disk)
    );
    assert_eq!(
        sensor_do_lhm("/lpc/nct6798d/temperature/1", "CPU"),
        Some(Sensor::Board)
    );
}

#[test]
fn fica_a_mais_quente_de_cada_sensor_em_ordem() {
    let leituras = [
        (Sensor::Disk, 3500),
        (Sensor::Cpu, 6000),
        (Sensor::Disk, 4100),
        (Sensor::Board, 3000),
    ];
    let quentes = mais_quentes(leituras);
    let pares: Vec<(Sensor, i16)> = quentes
        .iter()
        .map(|t| (t.sensor, t.centi_celsius))
        .collect();
    assert_eq!(
        pares,
        vec![
            (Sensor::Cpu, 6000),
            (Sensor::Disk, 4100),
            (Sensor::Board, 3000)
        ]
    );
}

#[test]
fn pid_stat_le_o_nome_com_parenteses_e_os_numeros_depois_dele() {
    let texto = "4812 (Web (Content)) S 1 4812 4812 0 -1 4194560 100 0 0 0 250 50 0 0 20 0 30 0 1234 2000000 512 18446744073709551615";
    assert_eq!(proc_pid_stat(texto), Some(("Web (Content)", 300, 512)));
    assert_eq!(proc_pid_stat("1 (curto) S 1"), None);
}
