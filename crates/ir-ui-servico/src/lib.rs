//! O serviço visto pela janela: o contrato [`servico::Servico`] e o [`simulado::ServicoSimulado`].
//!
//! Saiu da `ir-ui` quando o painel no Android a levou ao teto de tamanho de crate
//! ([09, §1](../../../docs/09-padroes-de-codigo.md)). A fronteira já estava provada: nenhum dos dois
//! módulos tocava no Slint, e o simulado só conhecia o contrato e o vocabulário do `ir-ipc`. A
//! `ir-ui` os reexporta com os nomes de sempre.

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

pub mod servico;
pub mod simulado;
