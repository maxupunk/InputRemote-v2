//! As placas de vídeo e as temperaturas prontas para desenhar.
//!
//! De cada parte da máquina sai o card de sempre (`temp-cpu`, `temp-gpu`…), com a mais quente
//! dela — as chaves não mudam, e o painel arrumado de quem já usa continua igual —, e um card por
//! sensor (`sensor:cpu:core-3`), que o aplicativo oferece em "Adicionar card" sem pôr na tela
//! sozinho: um processador de dezesseis núcleos não pode encher o painel por conta própria.

use crate::amostra::{Gpu, GpuVendor, Sensor, Temperature};

use super::{Bloco, decimal, kib, milesimos, porcento, vazio};
use crate::nivel;

/// O começo da chave de um card de sensor: `sensor:cpu:core-3`.
pub const PREFIXO_DE_SENSOR: &str = "sensor:";

/// Os cards das placas de vídeo: `gpu`, e `gpu-2` quando há duas.
pub(super) fn gpus(gpus: &[Gpu]) -> Vec<Bloco> {
    let duas = gpus.len() > 1;
    gpus.iter()
        .enumerate()
        .map(|(posicao, gpu)| {
            let chave = if posicao == 0 {
                "gpu".to_owned()
            } else {
                format!("gpu-{}", posicao + 1)
            };
            let rotulo = match (duas, fabricante(gpu.vendor)) {
                (true, Some(nome)) => format!("GPU · {nome}"),
                (true, None) => format!("GPU {}", posicao + 1),
                (false, _) => "GPU".to_owned(),
            };
            placa(&chave, &rotulo, gpu)
        })
        .collect()
}

fn placa(chave: &str, rotulo: &str, gpu: &Gpu) -> Bloco {
    let memoria = gpu
        .memory
        .map(|m| format!("{} de {}", kib(m.used_kib), kib(m.total_kib)));
    let Some(uso) = gpu.busy_permille else {
        // Sem contador de uso (a Intel no Linux): a memória, se houver, ainda diz algo.
        let detalhe =
            memoria.or_else(|| fabricante(gpu.vendor).map(|f| format!("{f}, sem leitura de uso")));
        return Bloco {
            detalhe,
            ..vazio(chave, rotulo)
        };
    };
    Bloco {
        valor: porcento(uso),
        detalhe: memoria.or_else(|| fabricante(gpu.vendor).map(str::to_owned)),
        fracao: Some(f32::from(uso) / 1000.0),
        // A placa de vídeo a 100 % num jogo é o normal dela; o que preocupa é a memória acabando.
        nivel: gpu.memory.map_or(nivel::Nivel::Normal, |m| {
            nivel::da_ocupacao(Some(milesimos(m)))
        }),
        serie: Some(f32::from(uso) / 10.0),
        ..vazio(chave, rotulo)
    }
}

const fn fabricante(vendor: GpuVendor) -> Option<&'static str> {
    match vendor {
        GpuVendor::Nvidia => Some("NVIDIA"),
        GpuVendor::Amd => Some("AMD"),
        GpuVendor::Intel => Some("Intel"),
        GpuVendor::Other => None,
    }
}

/// O card de cada parte (com a mais quente dela) e o de cada sensor.
pub(super) fn temperaturas(temperaturas: &[Temperature]) -> Vec<Bloco> {
    let mut partes: Vec<&Temperature> = Vec::new();
    for temperatura in temperaturas {
        match partes.iter_mut().find(|t| t.sensor == temperatura.sensor) {
            Some(atual) if atual.centi_celsius < temperatura.centi_celsius => *atual = temperatura,
            Some(_) => {}
            None => partes.push(temperatura),
        }
    }
    partes.sort_by_key(|t| t.sensor);
    let da_parte = partes.into_iter().map(|t| {
        let (chave, rotulo) = parte(t.sensor);
        // Com um sensor só na parte, o nome dele não acrescenta nada ao card.
        let varios = temperaturas.iter().filter(|o| o.sensor == t.sensor).count() > 1;
        Bloco {
            detalhe: varios.then(|| format!("mais quente: {}", t.name)),
            ..grau(chave, rotulo, t.centi_celsius)
        }
    });
    let de_cada = temperaturas.iter().map(|t| {
        let rotulo = rotulo_do_sensor(t);
        grau(&chave_do_sensor(t), &rotulo, t.centi_celsius)
    });
    da_parte.chain(de_cada).collect()
}

/// A chave do card de um sensor: a parte e o nome, em minúsculas e sem espaço.
#[must_use]
pub fn chave_do_sensor(temperatura: &Temperature) -> String {
    let (_, nome_da_parte) = parte_curta(temperatura.sensor);
    let nome: String = temperatura
        .name
        .to_lowercase()
        .chars()
        .filter_map(|c| match c {
            c if c.is_alphanumeric() => Some(c),
            ' ' | '-' | '_' | '.' => Some('-'),
            _ => None,
        })
        .collect();
    format!("{PREFIXO_DE_SENSOR}{nome_da_parte}:{nome}")
}

/// "CPU · Core #3"; um nome que já diz a parte ("GPU Hot Spot", "Disco 2") fica como está.
fn rotulo_do_sensor(temperatura: &Temperature) -> String {
    let (prefixo, _) = parte_curta(temperatura.sensor);
    let nome = temperatura.name.as_str();
    if nome.to_lowercase().starts_with(&prefixo.to_lowercase()) {
        nome.to_owned()
    } else {
        format!("{prefixo} · {nome}")
    }
}

const fn parte(sensor: Sensor) -> (&'static str, &'static str) {
    match sensor {
        Sensor::Cpu => ("temp-cpu", "Temp. CPU"),
        Sensor::Gpu => ("temp-gpu", "Temp. GPU"),
        Sensor::Disk => ("temp-disco", "Temp. disco"),
        Sensor::Board => ("temp-placa", "Temp. placa"),
        Sensor::Other => ("temp-outros", "Temp. outros"),
    }
}

/// O nome curto da parte, para o rótulo e para a chave.
const fn parte_curta(sensor: Sensor) -> (&'static str, &'static str) {
    match sensor {
        Sensor::Cpu => ("CPU", "cpu"),
        Sensor::Gpu => ("GPU", "gpu"),
        Sensor::Disk => ("Disco", "disco"),
        Sensor::Board => ("Placa", "placa"),
        Sensor::Other => ("Sensor", "outro"),
    }
}

fn grau(chave: &str, rotulo: &str, centi: i16) -> Bloco {
    let graus = f32::from(centi) / 100.0;
    Bloco {
        valor: format!("{} °C", decimal(f64::from(graus), 0)),
        nivel: nivel::da_temperatura(centi),
        serie: Some(graus),
        ..vazio(chave, rotulo)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::amostra::Occupancy;

    fn temperatura(sensor: Sensor, nome: &str, graus: i16) -> Temperature {
        Temperature {
            sensor,
            centi_celsius: graus * 100,
            name: nome.to_owned(),
        }
    }

    fn chaves(blocos: &[Bloco]) -> Vec<&str> {
        blocos.iter().map(|b| b.chave.as_str()).collect()
    }

    #[test]
    fn a_parte_mostra_a_mais_quente_e_cada_sensor_tem_card() {
        let blocos = temperaturas(&[
            temperatura(Sensor::Cpu, "Core #2", 55),
            temperatura(Sensor::Cpu, "Package", 61),
            temperatura(Sensor::Disk, "Disco 1", 38),
        ]);
        assert_eq!(
            chaves(&blocos),
            vec![
                "temp-cpu",
                "temp-disco",
                "sensor:cpu:core-2",
                "sensor:cpu:package",
                "sensor:disco:disco-1"
            ]
        );
        assert_eq!(blocos[0].valor, "61 °C");
        assert_eq!(blocos[0].detalhe.as_deref(), Some("mais quente: Package"));
        assert_eq!(
            blocos[1].detalhe, None,
            "um sensor só não precisa dizer o nome"
        );
        assert_eq!(blocos[2].rotulo, "CPU · Core #2");
        assert_eq!(blocos[4].rotulo, "Disco 1");
    }

    #[test]
    fn uma_placa_so_e_gpu_e_duas_dizem_o_fabricante() {
        let nvidia = Gpu {
            vendor: GpuVendor::Nvidia,
            busy_permille: Some(370),
            memory: Some(Occupancy {
                used_kib: 1024 * 1024,
                total_kib: 4 * 1024 * 1024,
            }),
        };
        let intel = Gpu {
            vendor: GpuVendor::Intel,
            busy_permille: None,
            memory: None,
        };
        let uma = gpus(&[nvidia]);
        assert_eq!(
            (uma[0].chave.as_str(), uma[0].rotulo.as_str()),
            ("gpu", "GPU")
        );
        assert_eq!(uma[0].valor, "37 %");
        assert_eq!(uma[0].detalhe.as_deref(), Some("1,0 GB de 4,0 GB"));
        let duas = gpus(&[nvidia, intel]);
        assert_eq!(duas[0].rotulo, "GPU · NVIDIA");
        assert_eq!(
            (duas[1].chave.as_str(), duas[1].rotulo.as_str()),
            ("gpu-2", "GPU · Intel")
        );
        assert_eq!(duas[1].valor, "—");
        assert_eq!(
            duas[1].detalhe.as_deref(),
            Some("Intel, sem leitura de uso")
        );
    }
}
