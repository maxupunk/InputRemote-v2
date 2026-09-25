//! O que todo processo do produto pede ao sistema, escrito uma vez.
//!
//! O serviço, o agente e o ajudante de clipboard são processos diferentes, com setas de dependência
//! diferentes, e cada um tinha a sua cópia de duas coisas que não são de nenhum deles: ligar o
//! registro ([`registro`]) e rodar uma ferramenta do sistema sem ficar preso a ela
//! ([`ferramenta`]). As cópias já tinham começado a divergir. Aqui elas são uma só, e este crate
//! não conhece nada do produto — por isso qualquer um pode depender dele.

#![forbid(unsafe_code)]

pub mod ferramenta;
pub mod registro;
