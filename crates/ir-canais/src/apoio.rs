//! O que os testes dos dois canais usam igual: um endereço que não colide e um cliente que insiste
//! até o servidor criar a instância.

#![allow(clippy::panic)]

/// Um endereço de teste único para esta execução, para dois testes não colidirem.
pub(crate) fn endereco_de_teste(rotulo: &str) -> String {
    let id = std::process::id();
    #[cfg(windows)]
    {
        format!(r"\\.\pipe\inputremote-test-{rotulo}-{id}")
    }
    #[cfg(not(windows))]
    {
        std::env::temp_dir()
            .join(format!("ir-test-{rotulo}-{id}.sock"))
            .to_string_lossy()
            .into_owned()
    }
}

/// Conecta como cliente, tentando por meio segundo: o servidor pode ainda não ter criado a
/// instância.
#[cfg(windows)]
pub(crate) async fn conectar(
    endereco: &str,
) -> impl tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin {
    use tokio::net::windows::named_pipe::ClientOptions;
    for _ in 0..50 {
        if let Ok(cliente) = ClientOptions::new().open(endereco) {
            return cliente;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    panic!("o cliente de teste não conectou em {endereco}");
}

/// Conecta como cliente, tentando por meio segundo: o servidor pode ainda não ter criado o socket.
#[cfg(not(windows))]
pub(crate) async fn conectar(
    endereco: &str,
) -> impl tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin {
    for _ in 0..50 {
        if let Ok(cliente) = tokio::net::UnixStream::connect(endereco).await {
            return cliente;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    panic!("o cliente de teste não conectou em {endereco}");
}
