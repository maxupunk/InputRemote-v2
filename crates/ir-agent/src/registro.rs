//! Para onde vai o registro do agente e do ajudante de clipboard.
//!
//! No Windows estes processos não têm console — o serviço os lança com `CREATE_NO_WINDOW`, e o
//! executável é de subsistema gráfico —, então a saída padrão deles não chega a lugar nenhum. Uma
//! falha do ajudante de clipboard ficava invisível, e a única pista era copiar e colar parar de
//! funcionar. Aqui ela vira arquivo, na pasta do usuário, um por dia.
//!
//! No Linux não é preciso: a unidade do `systemd` do usuário já manda a saída padrão ao `journald`,
//! que é onde quem administra a máquina procura.

/// Liga o registro e devolve o guarda que esvazia a fila ao sair.
///
/// Com nível de `RUST_LOG` ou `info` por padrão.
pub(crate) fn iniciar() -> tracing_appender::non_blocking::WorkerGuard {
    let (destino, terminal) = destino_do_registro();
    let (escritor, guarda) = tracing_appender::non_blocking(destino);
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .with_writer(escritor)
        .with_ansi(terminal)
        .init();
    guarda
}

/// Para onde vai o registro, e se o destino é um terminal.
///
/// No Windows, para `%LOCALAPPDATA%\InputRemote\logs`: este processo não tem console — o serviço o
/// lança com `CREATE_NO_WINDOW` —, então a saída padrão dele não chega a lugar nenhum. Foi assim que
/// uma falha do ajudante de clipboard ficou invisível. No Linux o `journald` já guarda a saída da
/// unidade do usuário, e ali a saída padrão é o lugar certo.
fn destino_do_registro() -> (Box<dyn std::io::Write + Send>, bool) {
    #[cfg(windows)]
    if let Some(arquivo) = arquivo_de_registro() {
        return (Box::new(arquivo), false);
    }
    let terminal = std::io::IsTerminal::is_terminal(&std::io::stdout());
    (Box::new(std::io::stdout()), terminal)
}

/// Quantos arquivos de registro diários o agente guarda.
#[cfg(windows)]
const DIAS_DE_REGISTRO: usize = 7;

/// O arquivo de registro na pasta do usuário, com um arquivo por dia.
#[cfg(windows)]
fn arquivo_de_registro() -> Option<tracing_appender::rolling::RollingFileAppender> {
    use tracing_appender::rolling::{RollingFileAppender, Rotation};
    let pasta = std::path::PathBuf::from(std::env::var_os("LOCALAPPDATA")?)
        .join("InputRemote")
        .join("logs");
    let nome = if std::env::args().any(|argumento| argumento == "--clipboard") {
        "clipboard"
    } else {
        "agente"
    };
    RollingFileAppender::builder()
        .rotation(Rotation::DAILY)
        .filename_prefix(nome)
        .filename_suffix("log")
        .max_log_files(DIAS_DE_REGISTRO)
        .build(pasta)
        .ok()
}
