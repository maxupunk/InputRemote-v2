//! Quando um número fica âmbar ou vermelho.
//!
//! Os limites da v1 são fixos e moram só aqui ([12, §5.2](../../../docs/12-painel-android.md)): o
//! aplicativo recebe o nível pronto e só escolhe a cor. Decididos em 2026-10-24.

use serde::Serialize;

/// O estado de um número na tela.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Nivel {
    /// Tudo bem.
    Normal,
    /// Vale olhar.
    Atencao,
    /// Fora do aceitável.
    Critico,
    /// Não há o número: a tela mostra "—".
    SemDado,
}

/// Processador: atenção a partir de 75 %, crítico a partir de 90 %.
#[must_use]
pub fn do_processador(permille: Option<u16>) -> Nivel {
    faixa(permille.map(u32::from), 750, 900)
}

/// Memória e disco: atenção a partir de 80 %, crítico a partir de 90 % de ocupação.
#[must_use]
pub fn da_ocupacao(permille: Option<u16>) -> Nivel {
    faixa(permille.map(u32::from), 800, 900)
}

/// Temperatura: atenção a partir de 75 °C, crítico a partir de 85 °C.
#[must_use]
pub fn da_temperatura(centi_celsius: i16) -> Nivel {
    faixa(u32::try_from(centi_celsius).ok().or(Some(0)), 7_500, 8_500)
}

/// Latência de uma rota: atenção a partir de 20 ms, crítico a partir de 50 ms.
///
/// 50 ms é onde o ponteiro começa a parecer arrastado; 20 ms já é o Wi-Fi cochilando
/// ([ADR-0013](../../../docs/adr/0013-economia-de-energia-do-wifi.md)). Rota sem medida não passa
/// por aqui: quem decide se ela está caída ou só ausente é a [`vista`](crate::vista).
#[must_use]
pub fn da_latencia(micros: u32) -> Nivel {
    faixa(Some(micros), 20_000, 50_000)
}

/// Bateria fora da tomada: atenção até 20 %, crítico até 10 %. Na tomada, sempre normal.
#[must_use]
pub const fn da_bateria(percent: u8, na_tomada: bool) -> Nivel {
    if na_tomada || percent > 20 {
        Nivel::Normal
    } else if percent > 10 {
        Nivel::Atencao
    } else {
        Nivel::Critico
    }
}

fn faixa(valor: Option<u32>, atencao: u32, critico: u32) -> Nivel {
    match valor {
        None => Nivel::SemDado,
        Some(v) if v >= critico => Nivel::Critico,
        Some(v) if v >= atencao => Nivel::Atencao,
        Some(_) => Nivel::Normal,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn processador_nas_bordas() {
        assert_eq!(do_processador(None), Nivel::SemDado);
        assert_eq!(do_processador(Some(749)), Nivel::Normal);
        assert_eq!(do_processador(Some(750)), Nivel::Atencao);
        assert_eq!(do_processador(Some(900)), Nivel::Critico);
    }

    #[test]
    fn ocupacao_nas_bordas() {
        assert_eq!(da_ocupacao(Some(799)), Nivel::Normal);
        assert_eq!(da_ocupacao(Some(800)), Nivel::Atencao);
        assert_eq!(da_ocupacao(Some(950)), Nivel::Critico);
    }

    #[test]
    fn temperatura_negativa_e_normal() {
        assert_eq!(da_temperatura(-500), Nivel::Normal);
        assert_eq!(da_temperatura(7_499), Nivel::Normal);
        assert_eq!(da_temperatura(7_500), Nivel::Atencao);
        assert_eq!(da_temperatura(8_500), Nivel::Critico);
    }

    #[test]
    fn latencia_nas_bordas() {
        assert_eq!(da_latencia(3_000), Nivel::Normal);
        assert_eq!(da_latencia(20_000), Nivel::Atencao);
        assert_eq!(da_latencia(50_000), Nivel::Critico);
    }

    #[test]
    fn bateria_na_tomada_nunca_alerta() {
        assert_eq!(da_bateria(5, true), Nivel::Normal);
        assert_eq!(da_bateria(21, false), Nivel::Normal);
        assert_eq!(da_bateria(20, false), Nivel::Atencao);
        assert_eq!(da_bateria(10, false), Nivel::Critico);
    }
}
