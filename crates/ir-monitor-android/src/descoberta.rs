//! Achar os computadores na rede: a mesma pergunta UDP de sempre ([`ir_net::descoberta`]).
//!
//! A descoberta do produto já sabe perguntar a cada sub-rede e repetir a pergunta que o Wi-Fi
//! perdeu; aqui ela só roda dentro de um `tokio` de uma linha, porque o resto do aplicativo é
//! bloqueante.

use std::net::{IpAddr, SocketAddr};
use std::time::Duration;

use serde::Serialize;

/// Um computador que respondeu.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Achado {
    /// Como ele se anuncia.
    pub nome: String,
    /// O identificador da instalação, para reconhecer um pareado de IP novo.
    #[serde(skip)]
    pub maquina: String,
    /// O IP de onde respondeu.
    pub endereco: IpAddr,
}

/// Pergunta à rede por `duracao` e devolve quem respondeu, sem repetir.
#[must_use]
pub fn procurar(duracao: Duration) -> Vec<Achado> {
    let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    else {
        return Vec::new();
    };
    let respostas = runtime
        .block_on(ir_net::descoberta::procurar(duracao))
        .unwrap_or_default();
    let mut achados: Vec<Achado> = Vec::new();
    for candidato in respostas {
        if achados.iter().any(|a| a.maquina == candidato.machine) {
            continue;
        }
        achados.push(Achado {
            nome: candidato.label,
            maquina: candidato.machine,
            endereco: candidato.addr.ip(),
        });
    }
    achados
}

/// Onde o painel atende num computador deste IP.
#[must_use]
pub const fn do_painel(ip: IpAddr) -> SocketAddr {
    SocketAddr::new(ip, ir_monitor::PORTA)
}
