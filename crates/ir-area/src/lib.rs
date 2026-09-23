//! O canal 4 — o texto do clipboard — visto de uma ponta, sem sessão, sem relógio e sem E/S.
//!
//! [`Area`] guarda o que esta ponta ofereceu, a fila dos pedaços a mandar e o que o par está
//! mandando; [`ClipText`] é o texto, com o limite do canal como invariante e sem nunca aparecer num
//! registro. Quem conduz — quando mandar, por qual portador, no ritmo da janela de confirmação — é
//! a sessão (`ir-session`), que só chama estas funções.
//!
//! Saiu do `ir-session` quando a sessão encostou no teto de 2 500 linhas de produção
//! ([09, §1](../../../docs/09-padroes-de-codigo.md)). A fronteira já estava provada: a montagem
//! tinha os próprios testes, que levavam um texto de uma área a outra sem sessão no meio.

#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

mod area;
mod texto;

pub use area::{Area, PEDACO};
pub use texto::ClipText;
