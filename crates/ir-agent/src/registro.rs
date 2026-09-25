//! Para onde vai o registro do agente e do ajudante de clipboard. Como ligá-lo é de
//! `ir_processo::registro`.
//!
//! No Windows estes processos não têm console — o serviço os lança com `CREATE_NO_WINDOW`, e o
//! executável é de subsistema gráfico —, então a saída padrão deles não chega a lugar nenhum. Uma
//! falha do ajudante de clipboard ficava invisível, e a única pista era copiar e colar parar de
//! funcionar. Aqui ela vira arquivo, na pasta do usuário, um por dia.
//!
//! No Linux não é preciso: a unidade do `systemd` do usuário já manda a saída padrão ao `journald`,
//! que é onde quem administra a máquina procura.

use ir_processo::registro::{Arquivo, WorkerGuard};

/// Liga o registro e devolve o guarda que esvazia a fila ao sair.
pub(crate) fn iniciar() -> WorkerGuard {
    ir_processo::registro::iniciar(arquivo_de_registro())
}

/// O arquivo diário em `%LOCALAPPDATA%\InputRemote\logs`, no Windows.
fn arquivo_de_registro() -> Option<Arquivo> {
    if !cfg!(windows) {
        return None;
    }
    let pasta = std::path::PathBuf::from(std::env::var_os("LOCALAPPDATA")?)
        .join("InputRemote")
        .join("logs");
    let prefixo = if std::env::args().any(|argumento| argumento == "--clipboard") {
        "clipboard"
    } else {
        "agente"
    };
    Some(Arquivo { pasta, prefixo })
}
