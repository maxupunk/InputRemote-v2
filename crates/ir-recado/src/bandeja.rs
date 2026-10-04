//! O ícone da bandeja do Windows: como ele fica pelo que acontece ([`aparencia`]), e os quadros que
//! o desenham ([`icones`]). Quem põe o ícone lá e o atende é a interface; aqui só se decide e se
//! desenha.

pub mod aparencia;
#[cfg(windows)]
pub mod icones;
