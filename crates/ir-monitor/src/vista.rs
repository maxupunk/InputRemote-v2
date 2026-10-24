//! O quadro pronto para desenhar: textos, frações e níveis.
//!
//! O aplicativo não conhece métrica nenhuma. Ele recebe uma [`Vista`] com blocos genéricos —
//! rótulo, valor, detalhe, quanto da barra encher, o nível e o número do mini-gráfico — e desenha
//! todos com o mesmo componente. Uma métrica nova é um bloco a mais aqui, e nenhuma linha lá.
//!
//! Os textos são em português do Brasil, com vírgula decimal.

use ir_proto::message::{Battery, Occupancy, Sensor, Temperature, Traffic, UsageSample};
use serde::Serialize;

use crate::mensagem::{Enlace, Maquina, Quadro, Rota, Situacao};
use crate::nivel::{self, Nivel};

/// O que o painel desenha.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Vista {
    /// Um cartão por computador.
    pub maquinas: Vec<VistaDaMaquina>,
    /// A faixa do enlace, embaixo dos cartões, quando há sessão entre os computadores.
    pub enlace: Option<VistaDoEnlace>,
}

/// O cartão de um computador.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct VistaDaMaquina {
    /// O nome.
    pub nome: String,
    /// Uma frase no lugar dos blocos, quando não há amostra: aguardando, desatualizado,
    /// desconectado.
    pub aviso: Option<String>,
    /// O nível do cartão: o pior entre os blocos, para a borda do cartão.
    pub nivel: Nivel,
    /// Os blocos, na ordem de desenhar.
    pub blocos: Vec<Bloco>,
}

/// A faixa do enlace entre os computadores.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct VistaDoEnlace {
    /// Um bloco por rota: Bluetooth e rede.
    pub rotas: Vec<Bloco>,
    /// "rede" ou "Bluetooth", quando as duas respondem e uma é mais rápida.
    pub vencendo: Option<String>,
}

/// Um número na tela.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Bloco {
    /// Estável entre os segundos: o aplicativo guarda o histórico do mini-gráfico por ela.
    pub chave: String,
    /// "CPU", "RAM", "Disco"…
    pub rotulo: String,
    /// O número, já formatado: "23 %", "61 °C", "—".
    pub valor: String,
    /// Uma linha a mais, menor: "11,2 de 32 GB", "na tomada".
    pub detalhe: Option<String>,
    /// Quanto da barra encher, de 0 a 1; `None` sem barra.
    pub fracao: Option<f32>,
    /// A cor.
    pub nivel: Nivel,
    /// O número do mini-gráfico deste segundo; `None` para não desenhar.
    pub serie: Option<f32>,
}

/// O texto de "não há este número".
pub const SEM_DADO: &str = "—";

impl Vista {
    /// A vista de um quadro.
    #[must_use]
    pub fn do_quadro(quadro: &Quadro) -> Self {
        Self {
            maquinas: quadro.maquinas.iter().map(maquina).collect(),
            enlace: quadro.enlace.as_ref().map(enlace),
        }
    }
}

fn maquina(maquina: &Maquina) -> VistaDaMaquina {
    let (aviso, blocos) = match &maquina.situacao {
        Situacao::Amostra(amostra) => (None, blocos(amostra)),
        Situacao::Aguardando => (Some("Aguardando os dados…"), Vec::new()),
        Situacao::VersaoAntiga => (
            Some("Atualize o InputRemote neste computador para ver os dados dele."),
            Vec::new(),
        ),
        Situacao::Desconectado => (Some("Desconectado."), Vec::new()),
    };
    VistaDaMaquina {
        nome: maquina.nome.clone(),
        aviso: aviso.map(str::to_owned),
        nivel: pior(&blocos),
        blocos,
    }
}

fn blocos(amostra: &UsageSample) -> Vec<Bloco> {
    let mut blocos = vec![
        processador(amostra.cpu_permille),
        ocupacao("ram", "RAM", amostra.memory),
        disco(amostra.disk),
        rede(amostra.network),
    ];
    blocos.extend(amostra.battery.map(bateria));
    if amostra.temperatures.is_empty() {
        blocos.push(Bloco {
            detalhe: Some("sem sensor legível".to_owned()),
            ..vazio("temp", "Temperatura")
        });
    }
    blocos.extend(amostra.temperatures.iter().copied().map(temperatura));
    blocos
}

fn processador(permille: Option<u16>) -> Bloco {
    let Some(permille) = permille else {
        return vazio("cpu", "CPU");
    };
    Bloco {
        valor: porcento(permille),
        fracao: Some(f32::from(permille) / 1000.0),
        nivel: nivel::do_processador(Some(permille)),
        serie: Some(f32::from(permille) / 10.0),
        ..vazio("cpu", "CPU")
    }
}

fn ocupacao(chave: &str, rotulo: &str, ocupacao: Option<Occupancy>) -> Bloco {
    let Some(ocupacao) = ocupacao else {
        return vazio(chave, rotulo);
    };
    let milesimos = milesimos(ocupacao);
    Bloco {
        valor: porcento(milesimos),
        detalhe: Some(format!(
            "{} de {}",
            kib(ocupacao.used_kib),
            kib(ocupacao.total_kib)
        )),
        fracao: Some(f32::from(milesimos) / 1000.0),
        nivel: nivel::da_ocupacao(Some(milesimos)),
        serie: Some(f32::from(milesimos) / 10.0),
        ..vazio(chave, rotulo)
    }
}

fn disco(disco: Option<Occupancy>) -> Bloco {
    let mut bloco = ocupacao("disco", "Disco", disco);
    if let Some(disco) = disco {
        let livre = disco.total_kib.saturating_sub(disco.used_kib);
        bloco.detalhe = Some(format!("{} livres", kib(livre)));
        // O disco muda devagar: um mini-gráfico dele seria uma reta.
        bloco.serie = None;
    }
    bloco
}

fn rede(trafego: Option<Traffic>) -> Bloco {
    let Some(trafego) = trafego else {
        return vazio("rede", "Rede");
    };
    let total = trafego.received_per_s.saturating_add(trafego.sent_per_s);
    Bloco {
        valor: format!("↓ {}", taxa(trafego.received_per_s)),
        detalhe: Some(format!("↑ {}", taxa(trafego.sent_per_s))),
        nivel: Nivel::Normal,
        #[allow(clippy::cast_precision_loss)]
        serie: Some(total as f32 / 1_000.0),
        ..vazio("rede", "Rede")
    }
}

fn bateria(bateria: Battery) -> Bloco {
    Bloco {
        valor: format!("{} %", bateria.percent),
        detalhe: Some(
            if bateria.plugged {
                "na tomada"
            } else {
                "na bateria"
            }
            .to_owned(),
        ),
        fracao: Some(f32::from(bateria.percent) / 100.0),
        nivel: nivel::da_bateria(bateria.percent, bateria.plugged),
        serie: None,
        ..vazio("bateria", "Bateria")
    }
}

fn temperatura(temperatura: Temperature) -> Bloco {
    let (chave, rotulo) = match temperatura.sensor {
        Sensor::Cpu => ("temp-cpu", "Temp. CPU"),
        Sensor::Gpu => ("temp-gpu", "Temp. GPU"),
        Sensor::Disk => ("temp-disco", "Temp. disco"),
        Sensor::Board => ("temp-placa", "Temp. placa"),
    };
    let graus = f32::from(temperatura.centi_celsius) / 100.0;
    Bloco {
        valor: format!("{} °C", decimal(f64::from(graus), 0)),
        nivel: nivel::da_temperatura(temperatura.centi_celsius),
        serie: Some(graus),
        ..vazio(chave, rotulo)
    }
}

fn enlace(enlace: &Enlace) -> VistaDoEnlace {
    // Sem nenhuma rota respondendo, o enlace caiu: vermelho. Sem uma só, ela não está na rota (a
    // máquina sem Bluetooth, por exemplo): "—", e não alarme.
    let caiu = enlace.bluetooth_us.is_none() && enlace.rede_us.is_none();
    let rota = |chave: &str, rotulo: &str, micros: Option<u32>| match micros {
        Some(us) => Bloco {
            valor: format!("{} ms", decimal(f64::from(us) / 1_000.0, 1)),
            nivel: nivel::da_latencia(us),
            #[allow(clippy::cast_precision_loss)]
            serie: Some(us as f32 / 1_000.0),
            ..vazio(chave, rotulo)
        },
        None => Bloco {
            nivel: if caiu { Nivel::Critico } else { Nivel::SemDado },
            detalhe: Some(if caiu { "sem resposta" } else { "fora da rota" }.to_owned()),
            ..vazio(chave, rotulo)
        },
    };
    let ambas = enlace.bluetooth_us.is_some() && enlace.rede_us.is_some();
    VistaDoEnlace {
        rotas: vec![
            rota("bluetooth", "Bluetooth", enlace.bluetooth_us),
            rota("rota-rede", "Rede", enlace.rede_us),
        ],
        vencendo: enlace.vencendo.filter(|_| ambas).map(|r| {
            match r {
                Rota::Bluetooth => "Bluetooth",
                Rota::Rede => "rede",
            }
            .to_owned()
        }),
    }
}

/// Um bloco sem número: "—", sem barra, sem gráfico.
fn vazio(chave: &str, rotulo: &str) -> Bloco {
    Bloco {
        chave: chave.to_owned(),
        rotulo: rotulo.to_owned(),
        valor: SEM_DADO.to_owned(),
        detalhe: None,
        fracao: None,
        nivel: Nivel::SemDado,
        serie: None,
    }
}

fn pior(blocos: &[Bloco]) -> Nivel {
    blocos
        .iter()
        .map(|b| b.nivel)
        .filter(|n| *n != Nivel::SemDado)
        .max()
        .unwrap_or(Nivel::Normal)
}

fn milesimos(ocupacao: Occupancy) -> u16 {
    if ocupacao.total_kib == 0 {
        return 0;
    }
    let usado = u128::from(ocupacao.used_kib.min(ocupacao.total_kib));
    u16::try_from(usado * 1000 / u128::from(ocupacao.total_kib)).unwrap_or(1000)
}

/// KiB em texto: "512 MB", "11,2 GB", "1,8 TB". Base 1024, com o nome que o sistema mostra.
fn kib(valor: u64) -> String {
    #[allow(clippy::cast_precision_loss)]
    let mut numero = valor as f64 / 1024.0;
    for unidade in ["MB", "GB", "TB"] {
        if numero < 1024.0 || unidade == "TB" {
            let casas = usize::from(numero < 100.0);
            return format!("{} {unidade}", decimal(numero, casas));
        }
        numero /= 1024.0;
    }
    SEM_DADO.to_owned()
}

/// Bytes por segundo em texto: "0 B/s", "80 KB/s", "1,2 MB/s". Base 1000, como a rede conta.
fn taxa(bytes_por_s: u64) -> String {
    #[allow(clippy::cast_precision_loss)]
    let mut numero = bytes_por_s as f64;
    for unidade in ["B/s", "KB/s", "MB/s", "GB/s"] {
        if numero < 1000.0 || unidade == "GB/s" {
            let casas = usize::from(unidade != "B/s" && numero < 100.0);
            return format!("{} {unidade}", decimal(numero, casas));
        }
        numero /= 1000.0;
    }
    SEM_DADO.to_owned()
}

/// Milésimos em porcentagem arredondada: 349,99 milésimos são "35 %", e não "34 %".
fn porcento(milesimos: u16) -> String {
    format!("{} %", (u32::from(milesimos) + 5) / 10)
}

/// Um número com `casas` decimais e vírgula.
fn decimal(numero: f64, casas: usize) -> String {
    format!("{numero:.casas$}").replace('.', ",")
}

#[cfg(test)]
mod tests;
