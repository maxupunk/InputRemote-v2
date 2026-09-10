//! Compila os arquivos `.slint` da interface.
//!
//! `ui/app.slint` é o ponto de entrada; ele importa os demais e reexporta o que o Rust enxerga.

/// Compila a interface.
///
/// # Errors
///
/// Devolve o erro do compilador do Slint, que aborta a compilação com a mensagem e a posição do
/// problema no `.slint`.
fn main() -> Result<(), slint_build::CompileError> {
    slint_build::compile("ui/app.slint")
}
