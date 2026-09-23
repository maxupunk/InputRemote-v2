//! Linux: `iw` para ler e desligar agora, e o `NetworkManager` para não voltar a cada reconexão.

use std::path::Path;

use crate::{Economia, ErroDeEnergia, rodar};

/// Onde o kernel lista as placas de rede; as de Wi-Fi têm um `wireless` dentro.
const PLACAS: &str = "/sys/class/net";

/// A configuração do `NetworkManager` que mantém a economia desligada.
const CONFIGURACAO: &str = "/etc/NetworkManager/conf.d/90-inputremote-wifi.conf";

/// `2` é "desligada" para o `NetworkManager`; o padrão das distribuições de notebook é `3`, ligada.
const CONTEUDO: &str = "\
# Criado pelo InputRemote, a pedido de quem clicou em Resolver na janela.
# Com a economia de energia ligada, o Wi-Fi cochila entre pacotes e o mouse pela rede trava em
# rajadas. Apague este arquivo para voltar ao padrão do sistema.
[connection]
wifi.powersave=2
";

/// As placas de Wi-Fi desta máquina.
fn placas_de_wifi() -> Vec<String> {
    let Ok(entradas) = std::fs::read_dir(PLACAS) else {
        return Vec::new();
    };
    entradas
        .flatten()
        .filter(|entrada| entrada.path().join("wireless").exists())
        .filter_map(|entrada| entrada.file_name().to_str().map(str::to_owned))
        .collect()
}

pub(crate) fn verificar() -> Economia {
    let placas = placas_de_wifi();
    if placas.is_empty() {
        return Economia::Desligada; // sem Wi-Fi não há o que cochilar
    }
    let mut resultado = Economia::Desligada;
    for placa in &placas {
        match rodar("iw", &["dev", placa, "get", "power_save"])
            .map(|saida| crate::economia_do_iw(&saida))
        {
            Ok(Economia::Ligada) => return Economia::Ligada,
            Ok(Economia::Desligada) => {}
            _ => resultado = Economia::Desconhecida,
        }
    }
    resultado
}

pub(crate) fn desligar() -> Result<(), ErroDeEnergia> {
    // Primeiro o que sobrevive: sem isto, a próxima reconexão do Wi-Fi volta a ligar a economia.
    if Path::new("/etc/NetworkManager").is_dir() {
        std::fs::write(CONFIGURACAO, CONTEUDO)
            .map_err(|erro| ErroDeEnergia(format!("{CONFIGURACAO}: {erro}")))?;
    }
    // Depois o que vale agora, placa por placa.
    for placa in placas_de_wifi() {
        rodar("iw", &["dev", &placa, "set", "power_save", "off"])?;
    }
    Ok(())
}
