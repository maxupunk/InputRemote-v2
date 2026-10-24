//! O painel no Android visto pelo serviço ([12](../../../docs/12-painel-android.md)).
//!
//! Decide, a cada segundo, o que o serviço precisa fazer pelo painel — e nada além disso:
//!
//! - **quando coletar**: a [`Vigia`](ir_telemetria::Vigia) só existe com um painel assinando, ou
//!   com o par pedindo a amostra daqui. Sem os dois, nada é lido;
//! - **o que pedir ao par**: a amostra dele, renovada a cada 2 s enquanto há assinante — o par a
//!   esquece em 5 s sem renovação, como o monitor esquece um painel;
//! - **que processos medir**: os que os painéis daqui querem, somados aos que o painel do par quer
//!   daqui; sem nenhum, os processos não são lidos;
//! - **a cópia em curso**: o mesmo aviso que a janela recebe, com a velocidade medida;
//! - **o pareamento e a lista**: o código para a janela, a chave para a configuração, os nomes.
//!
//! O serviço chama [`Vigilancia::notar`], [`Vigilancia::evento`], [`Vigilancia::batida`] e
//! [`Vigilancia::comando`], e só dirige a sessão com as entradas que voltam: os avisos à janela e a
//! gravação da configuração a vigilância faz sozinha, pelo mesmo emissor e pelo mesmo
//! [`Gravador`](ir_configuracao::Gravador) do serviço. A sessão é vista pela interface [`Sessao`],
//! que o teste falsifica.
//!
//! Saiu do `ir-daemon` antes de entrar nele: o serviço estava a duas linhas do teto de tamanho de
//! crate ([09, §1](../../../docs/09-padroes-de-codigo.md)).

#![forbid(unsafe_code)]
#![cfg_attr(
    test,
    allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::indexing_slicing,
        clippy::panic
    )
)]

mod lista;
mod vigilancia;

use ir_ipc::Transferencia;
use ir_monitor::Processos;
use ir_proto::message::UsageSample;

pub use vigilancia::Vigilancia;

/// O que chega ao serviço de fora do laço dele, para ele passar a [`Vigilancia::evento`].
#[derive(Debug, Clone)]
pub enum Evento {
    /// Um aviso do monitor: código, pareado, assinantes…
    Monitor(ir_monitor_rede::Aviso),
    /// A amostra de uso desta máquina, deste segundo, com os processos quando alguém os quer.
    Amostra(UsageSample, Option<Processos>),
    /// Uma cópia de arquivos entre os computadores andou.
    Copia(Transferencia),
}

/// A sessão, no que importa ao painel.
pub trait Sessao {
    /// O nome do par e se ele entende o recado do painel (versão 11), quando há sessão de pé.
    fn par(&self) -> Option<(&str, bool)>;
}

impl Sessao for ir_session::Session {
    fn par(&self) -> Option<(&str, bool)> {
        if !self.phase().is_established() {
            return None;
        }
        let par = self.peer()?;
        Some((par.name.as_str(), self.peer_has_panel()))
    }
}
