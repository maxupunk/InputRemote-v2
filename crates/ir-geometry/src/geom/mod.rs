//! Ponto e retângulo, em pixels lógicos do desktop virtual.
//!
//! Tudo aqui é inteiro. Não há ponto flutuante em nenhum lugar desta biblioteca, e isso é
//! deliberado: uma coordenada de ponteiro é um pixel, não uma fração dele, e `f32` não tem
//! ordenação total nem resultado exatamente reprodutível entre plataformas — o que
//! quebraria os vetores gravados e tornaria um teste de mapeamento frágil.

pub(crate) mod point;
pub(crate) mod rect;
mod scale;

pub use point::Point;
pub use rect::Rect;
