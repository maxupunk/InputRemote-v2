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
fn numero_do_sys_em_decimal_ou_hexadecimal() {
    assert_eq!(numero_do_sys("0x1002\n"), Some(0x1002));
    assert_eq!(numero_do_sys("4294967296"), Some(4_294_967_296));
    assert_eq!(numero_do_sys("lixo"), None);
}
