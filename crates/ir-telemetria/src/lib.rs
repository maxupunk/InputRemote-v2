//! A amostra de uso desta máquina, para o painel no Android ([12](../../../docs/12-painel-android.md)).
//!
//! O serviço liga a [`Vigia`] quando um painel começa a olhar e a solta quando ele para. Enquanto
//! ela vive, uma linha própria tira uma [`UsageSample`](ir_proto::message::UsageSample) por segundo
//! e a entrega a quem pediu. Sem vigia não há linha, nem arquivo aberto, nem consulta ao WMI: o
//! custo do painel é zero quando ninguém olha (§3.1 do documento).
//!
//! # Organização
//!
//! | Módulo | O quê |
//! |---|---|
//! | [`fonte`] | a interface de uma métrica, e a cadência de cada uma |
//! | [`coletor`] | junta as fontes numa amostra, sem E/S própria |
//! | [`conta`] | as contas: uso do processador e taxa de bytes entre duas leituras |
//! | [`leitura`] | o texto de `/proc` e `/sys` virando número, puro e testável em qualquer sistema |
//! | `linux`, `windows` | as fontes de cada sistema |
//! | [`processos`] | os processos agrupados pelo nome, entre duas fotos do sistema |
//! | [`vigia`] | a linha que amostra a cada segundo |
//!
//! Cada fonte lê **uma** coisa e escreve **um** campo da amostra. O coletor não sabe de sistema
//! operacional, e uma fonte nova (uma GPU por NVML, por exemplo) entra sem mexer nele.

#![cfg_attr(
    test,
    allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::indexing_slicing,
        clippy::panic
    )
)]

pub mod coletor;
pub mod conta;
pub mod fonte;
pub mod leitura;
pub mod processos;
pub mod vigia;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(windows)]
mod windows;

pub use coletor::Coletor;
pub use fonte::{Agendada, Fonte};
pub use processos::Medidor;
pub use vigia::Vigia;

/// As fontes desta máquina, cada uma com a sua cadência.
///
/// O que o sistema não deixa ler simplesmente não entra: a amostra sai com o campo vazio, e a tela
/// mostra "—".
#[must_use]
pub fn fontes_desta_maquina() -> Vec<Agendada> {
    #[cfg(target_os = "linux")]
    {
        linux::fontes()
    }
    #[cfg(windows)]
    {
        windows::fontes()
    }
    #[cfg(not(any(target_os = "linux", windows)))]
    {
        Vec::new()
    }
}

/// O medidor dos processos desta máquina; `None` onde não se sabe tirar a foto.
#[must_use]
pub fn medidor_desta_maquina() -> Option<Medidor> {
    #[cfg(target_os = "linux")]
    {
        linux::Proc::abrir().map(Medidor::novo)
    }
    #[cfg(windows)]
    {
        Some(Medidor::novo(windows::ListaDoSistema::default()))
    }
    #[cfg(not(any(target_os = "linux", windows)))]
    {
        None
    }
}
