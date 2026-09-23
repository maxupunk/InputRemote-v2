//! Windows: o "modo de economia de energia" do adaptador sem fio, no plano de energia ativo.

use crate::{Economia, ErroDeEnergia, rodar};

/// O subgrupo "Configurações do adaptador sem fio".
const SUBGRUPO: &str = "19cbb8fa-5279-450e-9fac-8a3d5fedd0c1";
/// A configuração "Modo de economia de energia" dentro dele.
const CONFIGURACAO: &str = "12bbebe6-58d6-4636-95bb-3217ef867c1a";

/// Se há adaptador sem fio. Cada um aparece com um `GUID` na listagem, em qualquer idioma.
fn ha_wifi() -> bool {
    rodar("netsh", &["wlan", "show", "interfaces"]).is_ok_and(|saida| saida.contains("GUID"))
}

pub(crate) fn verificar() -> Economia {
    if !ha_wifi() {
        return Economia::Desligada;
    }
    rodar(
        "powercfg",
        &["/q", "SCHEME_CURRENT", SUBGRUPO, CONFIGURACAO],
    )
    .map_or(Economia::Desconhecida, |saida| {
        crate::economia_do_powercfg(&saida)
    })
}

pub(crate) fn desligar() -> Result<(), ErroDeEnergia> {
    // Zero é "desempenho máximo", na tomada e na bateria; `setactive` faz o plano reler os valores.
    for comando in ["/setacvalueindex", "/setdcvalueindex"] {
        rodar(
            "powercfg",
            &[comando, "SCHEME_CURRENT", SUBGRUPO, CONFIGURACAO, "0"],
        )?;
    }
    rodar("powercfg", &["/setactive", "SCHEME_CURRENT"])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    /// Só lê; roda com `cargo test -p ir-energia -- --ignored` numa máquina com Wi-Fi.
    #[test]
    #[ignore = "lê a configuração de energia desta máquina"]
    fn le_a_economia_desta_maquina() {
        println!("economia do Wi-Fi: {:?}", super::verificar());
    }
}
