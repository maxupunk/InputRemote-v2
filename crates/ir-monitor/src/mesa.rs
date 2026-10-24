//! O computador que agrega: a amostra dele, a do par e as voltas de cada rota, num [`Quadro`].
//!
//! Sem E/S e sem relógio próprio: o instante entra por parâmetro, como no núcleo.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use ir_proto::message::UsageSample;

use crate::mensagem::{Enlace, Maquina, Quadro, Rota, Situacao};

/// A janela das voltas: a mediana dos últimos 10 s, a mesma janela da latência da janela do
/// computador ([01, §5](../../../docs/01-visao-e-escopo.md)).
pub const JANELA: Duration = Duration::from_secs(10);

/// Depois de quanto tempo sem amostra o par volta a "aguardando". Três amostras perdidas: uma só
/// atrasada não deve piscar a tela.
pub const AMOSTRA_VELHA: Duration = Duration::from_secs(3);

/// Quantas voltas guardar por rota. Uma batida a cada 200 ms dá 50 em 10 s; o dobro é folga.
const CAPACIDADE: usize = 100;

/// O que o computador sabe para montar o quadro.
#[derive(Debug)]
pub struct Mesa {
    nome: String,
    local: Option<UsageSample>,
    par: Par,
    bluetooth: Voltas,
    rede: Voltas,
}

#[derive(Debug)]
enum Par {
    Nenhum,
    Antigo(String),
    Conectado {
        nome: String,
        amostra: Option<(Instant, UsageSample)>,
    },
}

impl Mesa {
    /// A mesa deste computador, com o nome que o painel mostra.
    #[must_use]
    pub fn nova(nome: impl Into<String>) -> Self {
        Self {
            nome: nome.into(),
            local: None,
            par: Par::Nenhum,
            bluetooth: Voltas::default(),
            rede: Voltas::default(),
        }
    }

    /// A amostra deste computador.
    pub fn anotar_local(&mut self, amostra: UsageSample) {
        self.local = Some(amostra);
    }

    /// A sessão com o par subiu. `entende` diz se ele manda amostra (versão 11 ou mais).
    pub fn par_conectado(&mut self, nome: impl Into<String>, entende: bool) {
        let nome = nome.into();
        self.par = if entende {
            Par::Conectado {
                nome,
                amostra: None,
            }
        } else {
            Par::Antigo(nome)
        };
    }

    /// A sessão com o par caiu: as voltas de antes não dizem nada da próxima.
    pub fn par_desconectado(&mut self) {
        self.par = Par::Nenhum;
        self.bluetooth = Voltas::default();
        self.rede = Voltas::default();
    }

    /// A amostra que o par mandou.
    pub fn anotar_par(&mut self, agora: Instant, nova: UsageSample) {
        if let Par::Conectado { amostra, .. } = &mut self.par {
            *amostra = Some((agora, nova));
        }
    }

    /// Uma volta medida por uma rota.
    pub fn anotar_volta(&mut self, rota: Rota, agora: Instant, micros: u32) {
        match rota {
            Rota::Bluetooth => self.bluetooth.anotar(agora, micros),
            Rota::Rede => self.rede.anotar(agora, micros),
        }
    }

    /// O quadro de agora.
    #[must_use]
    pub fn quadro(&self, agora: Instant) -> Quadro {
        let mut maquinas = vec![Maquina {
            nome: self.nome.clone(),
            situacao: self
                .local
                .clone()
                .map_or(Situacao::Aguardando, Situacao::Amostra),
        }];
        let enlace = match &self.par {
            Par::Nenhum => None,
            Par::Antigo(nome) => {
                maquinas.push(maquina(nome, Situacao::VersaoAntiga));
                Some(self.enlace(agora))
            }
            Par::Conectado { nome, amostra } => {
                let situacao = match amostra {
                    Some((quando, amostra))
                        if agora.saturating_duration_since(*quando) < AMOSTRA_VELHA =>
                    {
                        Situacao::Amostra(amostra.clone())
                    }
                    _ => Situacao::Aguardando,
                };
                maquinas.push(maquina(nome, situacao));
                Some(self.enlace(agora))
            }
        };
        Quadro { maquinas, enlace }
    }

    fn enlace(&self, agora: Instant) -> Enlace {
        let bluetooth_us = self.bluetooth.mediana(agora);
        let rede_us = self.rede.mediana(agora);
        let vencendo = match (bluetooth_us, rede_us) {
            (Some(bt), Some(rede)) if bt < rede => Some(Rota::Bluetooth),
            (Some(_) | None, Some(_)) => Some(Rota::Rede),
            (Some(_), None) => Some(Rota::Bluetooth),
            (None, None) => None,
        };
        Enlace {
            bluetooth_us,
            rede_us,
            vencendo,
        }
    }
}

fn maquina(nome: &str, situacao: Situacao) -> Maquina {
    Maquina {
        nome: nome.to_owned(),
        situacao,
    }
}

/// As voltas de uma rota na última [`JANELA`].
#[derive(Debug, Default)]
struct Voltas(VecDeque<(Instant, u32)>);

impl Voltas {
    fn anotar(&mut self, agora: Instant, micros: u32) {
        while self.0.len() >= CAPACIDADE
            || self
                .0
                .front()
                .is_some_and(|(quando, _)| agora.saturating_duration_since(*quando) > JANELA)
        {
            self.0.pop_front();
        }
        self.0.push_back((agora, micros));
    }

    fn mediana(&self, agora: Instant) -> Option<u32> {
        let mut valores: Vec<u32> = self
            .0
            .iter()
            .filter(|(quando, _)| agora.saturating_duration_since(*quando) <= JANELA)
            .map(|(_, us)| *us)
            .collect();
        valores.sort_unstable();
        valores.get(valores.len() / 2).copied()
    }
}

#[cfg(test)]
mod tests;
