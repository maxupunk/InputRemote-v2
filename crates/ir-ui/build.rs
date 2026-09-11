//! Compila os arquivos `.slint` da interface e embute o icone no executavel.
//!
//! `ui/app.slint` e o ponto de entrada; ele importa os demais e reexporta o que o Rust enxerga.

/// Prepara o que o executavel precisa antes de compilar.
///
/// # Errors
///
/// Devolve o erro do compilador do Slint, que aborta a compilacao com a mensagem e a posicao do
/// problema no `.slint`, ou o erro do compilador de recursos do Windows.
fn main() -> Result<(), Box<dyn std::error::Error>> {
    embutir_icone()?;
    slint_build::compile("ui/app.slint")?;
    Ok(())
}

/// Embute o icone e a identificacao do programa no `.exe`.
///
/// Sem isto o Explorer, a barra de tarefas e a lista de programas mostram o icone generico. O
/// icone que a **janela** usa e outro caminho, definido em `ui/app.slint`: um e do arquivo, o
/// outro e da janela, e os dois precisam existir.
#[cfg(windows)]
fn embutir_icone() -> Result<(), Box<dyn std::error::Error>> {
    // `cfg(windows)` num build script fala da maquina que compila, nao do alvo. Numa compilacao
    // cruzada de Windows para Linux, embutir recurso de Windows produziria um binario invalido.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return Ok(());
    }

    println!("cargo:rerun-if-changed=../../recursos/icone.ico");

    let mut recurso = winresource::WindowsResource::new();
    recurso.set_icon("../../recursos/icone.ico");
    recurso.set("ProductName", "InputRemote");
    recurso.set(
        "FileDescription",
        "Compartilha teclado e mouse com outro computador",
    );
    recurso.set("LegalCopyright", "Projeto InputRemote - MIT");
    recurso.compile()?;
    Ok(())
}

/// Fora do Windows nao ha recurso a embutir.
///
/// Devolve `Result` mesmo sem ter como falhar porque a assinatura precisa casar com a da versao
/// do Windows: e isso que mantem o `main` identico nos dois, em vez de um `cfg` no meio dele.
#[cfg(not(windows))]
#[allow(clippy::unnecessary_wraps)]
fn embutir_icone() -> Result<(), Box<dyn std::error::Error>> {
    Ok(())
}
