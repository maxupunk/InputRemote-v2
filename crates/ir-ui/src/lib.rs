//! A janela de configuração do InputRemote.
//!
//! Esta crate não conhece o produto. Ela conhece [`ir_ipc`], que é um contrato publicado, e mais
//! nada: não sabe o que é RFCOMM, não sabe o que é um quadro, não sabe injetar tecla nenhuma.
//! Essa fronteira é o motivo de ela existir separada, e a lição mais cara do v1 — lá a interface
//! virou o produto, com 10.491 linhas, mais que transporte e plataforma somados
//! ([00, §1](../../../docs/00-licoes-do-v1.md)).
//!
//! É biblioteca **e** executável de propósito: a biblioteca é o que os testes conduzem, o
//! executável é só a escolha de qual serviço usar.

#![cfg_attr(
    test,
    allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::indexing_slicing,
        clippy::panic
    )
)]

/// O código que o compilador do Slint gera a partir de `ui/*.slint`.
///
/// Isolado num módulo com as verificações desligadas porque não é código nosso. Aplicar as
/// nossas regras de estilo a código gerado só produziria avisos que não temos como consertar.
pub mod gerado {
    #![allow(
        unsafe_code,
        missing_docs,
        missing_debug_implementations,
        unreachable_pub,
        clippy::all,
        clippy::pedantic,
        clippy::indexing_slicing,
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::todo,
        clippy::unimplemented,
        unused
    )]

    slint::include_modules!();
}

pub mod conector;
pub mod conexao;
pub mod janela;
pub mod ponte;
pub mod real;
pub mod servico;
pub mod simulado;
