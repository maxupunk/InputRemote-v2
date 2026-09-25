//! O registro do serviço: para onde vai. Como ligá-lo é de `ir_processo::registro`.

use ir_processo::registro::{Arquivo, WorkerGuard};

/// Liga o registro do serviço e devolve o guarda que esvazia a fila ao sair.
///
/// Como serviço do Windows, para arquivos diários em `pasta` (`%ProgramData%\InputRemote\logs`):
/// ninguém lê a saída padrão de um serviço, e um serviço que falha sem deixar rastro não tem como
/// ser diagnosticado. Em primeiro plano, e no Linux — onde o `journald` já guarda a saída do
/// serviço —, para a saída padrão.
pub fn iniciar(pasta: &std::path::Path) -> WorkerGuard {
    let arquivo = cfg!(windows).then(|| Arquivo {
        pasta: pasta.to_path_buf(),
        prefixo: "inputremote",
    });
    ir_processo::registro::iniciar(arquivo.filter(|_| como_servico()))
}

/// Se este processo roda como serviço do Windows.
fn como_servico() -> bool {
    #[cfg(windows)]
    {
        ir_sessao::como_servico()
    }
    #[cfg(not(windows))]
    {
        false
    }
}
