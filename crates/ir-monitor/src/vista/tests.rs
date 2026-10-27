use super::*;
use crate::amostra::{Gpu, GpuVendor, Sensor, Temperature};

fn completa() -> UsageSample {
    UsageSample {
        cpu_permille: Some(231),
        memory: Some(Occupancy {
            used_kib: 11_744_051,
            total_kib: 33_554_432,
        }),
        disk: Some(Occupancy {
            used_kib: 500_000_000,
            total_kib: 932_000_000,
        }),
        network: Some(Traffic {
            received_per_s: 1_260_000,
            sent_per_s: 80_000,
        }),
        battery: Some(Battery {
            percent: 15,
            plugged: false,
        }),
        temperatures: vec![Temperature {
            sensor: Sensor::Cpu,
            centi_celsius: 6150,
            name: "Package".to_owned(),
        }],
        gpus: vec![Gpu {
            vendor: GpuVendor::Nvidia,
            busy_permille: Some(420),
            memory: None,
        }],
    }
}

fn bloco<'a>(vista: &'a VistaDaMaquina, chave: &str) -> &'a Bloco {
    vista.blocos.iter().find(|b| b.chave == chave).unwrap()
}

fn uma_maquina(situacao: Situacao) -> VistaDaMaquina {
    let quadro = Quadro {
        maquinas: vec![Maquina {
            nome: "DESKTOP".to_owned(),
            situacao,
        }],
        enlace: None,
    };
    Vista::do_quadro(&quadro).maquinas.remove(0)
}

#[test]
fn os_numeros_saem_no_formato_brasileiro() {
    let vista = uma_maquina(Situacao::Amostra(completa()));
    assert_eq!(bloco(&vista, "cpu").valor, "23 %");
    assert_eq!(bloco(&vista, "ram").valor, "35 %");
    assert_eq!(
        bloco(&vista, "ram").detalhe.as_deref(),
        Some("11,2 GB de 32,0 GB")
    );
    assert_eq!(
        bloco(&vista, "disco").detalhe.as_deref(),
        Some("412 GB livres")
    );
    assert_eq!(bloco(&vista, "rede").valor, "↓ 1,3 MB/s");
    assert_eq!(
        bloco(&vista, "rede").detalhe.as_deref(),
        Some("↑ 80,0 KB/s")
    );
    assert_eq!(bloco(&vista, "temp-cpu").valor, "62 °C");
    assert_eq!(
        bloco(&vista, "bateria").detalhe.as_deref(),
        Some("na bateria")
    );
}

#[test]
fn o_cartao_toma_o_pior_nivel_dos_blocos() {
    let vista = uma_maquina(Situacao::Amostra(completa()));
    assert_eq!(bloco(&vista, "bateria").nivel, Nivel::Atencao);
    assert_eq!(vista.nivel, Nivel::Atencao);
}

#[test]
fn campo_que_falta_vira_traco_e_nao_zero() {
    let vista = uma_maquina(Situacao::Amostra(UsageSample::default()));
    let cpu = bloco(&vista, "cpu");
    assert_eq!(cpu.valor, SEM_DADO);
    assert_eq!(cpu.nivel, Nivel::SemDado);
    assert!(cpu.fracao.is_none());
    assert_eq!(
        bloco(&vista, "temp").detalhe.as_deref(),
        Some("sem sensor legível")
    );
    assert!(
        vista.blocos.iter().all(|b| b.chave != "bateria"),
        "desktop sem bateria"
    );
    assert_eq!(vista.nivel, Nivel::Normal);
}

#[test]
fn sem_amostra_o_cartao_diz_por_que() {
    let antigo = uma_maquina(Situacao::VersaoAntiga);
    assert!(antigo.aviso.unwrap().contains("Atualize"));
    assert!(antigo.blocos.is_empty());
    assert!(uma_maquina(Situacao::Aguardando).aviso.is_some());
}

#[test]
fn rota_ausente_e_traco_mas_as_duas_ausentes_e_vermelho() {
    let so_rede = enlace(&Enlace {
        bluetooth_us: None,
        rede_us: Some(3_200),
        vencendo: Some(Rota::Rede),
    });
    assert_eq!(so_rede.rotas[0].nivel, Nivel::SemDado);
    assert_eq!(so_rede.rotas[1].valor, "3,2 ms");
    assert_eq!(so_rede.vencendo, None, "com uma rota só, ninguém vence");

    let caiu = enlace(&Enlace::default());
    assert!(caiu.rotas.iter().all(|r| r.nivel == Nivel::Critico));
}

#[test]
fn com_as_duas_rotas_diz_quem_vence() {
    let ambas = enlace(&Enlace {
        bluetooth_us: Some(7_000),
        rede_us: Some(3_000),
        vencendo: Some(Rota::Rede),
    });
    assert_eq!(ambas.vencendo.as_deref(), Some("rede"));
}

#[test]
fn unidades_nas_bordas() {
    assert_eq!(kib(512 * 1024), "512 MB");
    assert_eq!(kib(2 * 1024 * 1024 * 1024), "2,0 TB");
    assert_eq!(taxa(0), "0 B/s");
    assert_eq!(taxa(999), "999 B/s");
    assert_eq!(taxa(150_000), "150 KB/s");
}

#[test]
fn o_json_e_o_contrato_com_o_aplicativo() {
    // O aplicativo lê estes nomes. Mudar um deles quebra a tela de lá em silêncio.
    let vista = uma_maquina(Situacao::Amostra(UsageSample::default()));
    let json = serde_json::to_value(&vista).unwrap();
    assert_eq!(json["nome"], "DESKTOP");
    assert_eq!(json["nivel"], "normal");
    let cpu = &json["blocos"][0];
    for campo in [
        "chave", "rotulo", "valor", "detalhe", "fracao", "nivel", "serie",
    ] {
        assert!(cpu.get(campo).is_some(), "falta `{campo}` no bloco");
    }
    assert_eq!(cpu["nivel"], "sem_dado");
    assert_eq!(serde_json::to_value(Nivel::Atencao).unwrap(), "atencao");
    assert_eq!(serde_json::to_value(Nivel::Critico).unwrap(), "critico");
}
