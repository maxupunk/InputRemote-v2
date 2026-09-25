//! Ligar o registro: com que nível, para onde, e sem nunca bloquear quem registra.

use std::path::PathBuf;

pub use tracing_appender::non_blocking::WorkerGuard;

/// Quantos arquivos de registro diários se guardam.
const DIAS_DE_REGISTRO: usize = 7;

/// Um registro em arquivo, um por dia, com os mais velhos apagados.
///
/// É o destino de quem não tem para onde escrever: no Windows, o serviço não tem console e o
/// agente nasce com `CREATE_NO_WINDOW`, então a saída padrão deles não chega a lugar nenhum. No
/// Linux o `journald` já guarda a saída padrão, e ali ninguém pede arquivo.
#[derive(Debug, Clone)]
pub struct Arquivo {
    /// A pasta dos arquivos.
    pub pasta: PathBuf,
    /// O começo do nome de cada arquivo (`inputremote`, `agente`…).
    pub prefixo: &'static str,
}

/// Liga o registro, com nível de `RUST_LOG` ou `info` por padrão, e devolve o guarda que esvazia a
/// fila ao sair — ele precisa viver até o fim.
///
/// O escritor é de fila: uma linha nunca espera o disco ou o console, porque quem registra pode ser
/// o laço da sessão, que bate a cada 5 ms. Sem `arquivo`, ou se ele não abrir, vai para a saída
/// padrão.
pub fn iniciar(arquivo: Option<Arquivo>) -> WorkerGuard {
    let (destino, terminal) = destino(arquivo);
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
fn destino(arquivo: Option<Arquivo>) -> (Box<dyn std::io::Write + Send>, bool) {
    if let Some(arquivo) = arquivo.and_then(abrir) {
        return (Box::new(arquivo), false);
    }
    let terminal = std::io::IsTerminal::is_terminal(&std::io::stdout());
    (Box::new(std::io::stdout()), terminal)
}

/// O arquivo diário, se a pasta aceitar.
fn abrir(arquivo: Arquivo) -> Option<tracing_appender::rolling::RollingFileAppender> {
    use tracing_appender::rolling::{RollingFileAppender, Rotation};
    RollingFileAppender::builder()
        .rotation(Rotation::DAILY)
        .filename_prefix(arquivo.prefixo)
        .filename_suffix("log")
        .max_log_files(DIAS_DE_REGISTRO)
        .build(arquivo.pasta)
        .ok()
}
