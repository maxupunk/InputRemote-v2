//! O registro do serviço: para onde vai, com que nível, e sem nunca bloquear quem registra.

/// Quantos arquivos de registro diários o serviço do Windows guarda.
#[cfg(windows)]
const DIAS_DE_REGISTRO: usize = 7;

/// Configura o registro, com nível de `RUST_LOG` ou `info` por padrão, sem bloquear quem registra.
///
/// O escritor é de fila: uma linha nunca espera o disco ou o console, porque quem registra pode ser
/// o laço da sessão, que bate a cada 5 ms. O guarda devolvido esvazia a fila ao sair, e precisa
/// viver até o fim.
///
/// `pasta` é para onde o serviço do Windows escreve os arquivos diários; fora dele, a saída padrão.
pub fn iniciar(pasta: &std::path::Path) -> tracing_appender::non_blocking::WorkerGuard {
    let (destino, terminal) = destino_do_registro(pasta);
    let (escritor, guarda) = tracing_appender::non_blocking(destino);
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .with_writer(escritor)
        // Cor só num terminal de verdade: num arquivo ou no `journald`, os códigos de cor viram
        // lixo no meio de cada linha.
        .with_ansi(terminal)
        .init();
    guarda
}

/// Para onde vai o registro, e se o destino é um terminal.
///
/// Como serviço do Windows, para `%ProgramData%\InputRemote\logs`: ninguém lê a saída padrão de um
/// serviço, e um serviço que falha sem deixar rastro não tem como ser diagnosticado. Em primeiro
/// plano, e no Linux — onde o `journald` já guarda a saída do serviço —, para a saída padrão.
fn destino_do_registro(pasta: &std::path::Path) -> (Box<dyn std::io::Write + Send>, bool) {
    #[cfg(windows)]
    {
        if ir_sessao::como_servico()
            && let Some(arquivo) = arquivo_de_registro(pasta)
        {
            return (Box::new(arquivo), false);
        }
    }
    #[cfg(not(windows))]
    let _ = pasta;
    let terminal = std::io::IsTerminal::is_terminal(&std::io::stdout());
    (Box::new(std::io::stdout()), terminal)
}

/// O arquivo de registro do serviço do Windows, com um arquivo por dia e os mais velhos apagados.
#[cfg(windows)]
fn arquivo_de_registro(
    pasta: &std::path::Path,
) -> Option<tracing_appender::rolling::RollingFileAppender> {
    use tracing_appender::rolling::{RollingFileAppender, Rotation};
    RollingFileAppender::builder()
        .rotation(Rotation::DAILY)
        .filename_prefix("inputremote")
        .filename_suffix("log")
        .max_log_files(DIAS_DE_REGISTRO)
        .build(pasta)
        .ok()
}
