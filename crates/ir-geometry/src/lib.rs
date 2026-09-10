//! Telas, bordas e mapeamento de coordenadas.
//!
//! Crate **puro**: nenhuma E/S, nenhum relógio, nenhuma API de sistema operacional, nenhum
//! ponto flutuante. Ele recebe o arranjo de telas que a outra ponta anunciou
//! (`ir_proto::screens`) e responde perguntas geométricas sobre ele.
//!
//! # O problema que este crate resolve
//!
//! Duas máquinas têm monitores diferentes, em quantidades diferentes, com resoluções e
//! escalas diferentes, e origens que podem ser negativas. Quando o ponteiro sai pela borda
//! de uma, ele precisa aparecer no lugar **proporcionalmente equivalente** da outra — senão
//! atravessar a borda parece um salto aleatório.
//!
//! A resposta é medir a saída como uma fração `0..=u16::MAX` ao longo da borda, e não em
//! pixels. Fração é independente de resolução: o meio da borda de um monitor 4K é o meio da
//! borda de um monitor 720p.
//!
//! # Por que não há ponto flutuante
//!
//! `f32` não tem ordenação total, arredonda de forma diferente entre plataformas e tornaria
//! um teste de mapeamento frágil. Toda a aritmética aqui é inteira, com `i64`/`u64` como
//! intermediário onde a multiplicação poderia estourar.
//!
//! # Garantia central
//!
//! Nenhuma função deste crate devolve uma coordenada que o injetor não possa usar. Ponto
//! fora de toda tela — porque um monitor foi desconectado, porque o arranjo mudou, ou porque
//! o par enviou lixo — é trazido para a tela mais próxima por [`desktop::Desktop::nearest_valid`].

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

pub mod crossing;
pub mod desktop;
pub mod geom;

pub use crossing::{Crossing, Movement, advance, entering_edge, entry_position};
pub use desktop::{Desktop, Monitor};
pub use geom::{Point, Rect};
