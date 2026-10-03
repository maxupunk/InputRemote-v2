//! As ações de disco que o motor pede, executadas de verdade.
//!
//! Três cuidados valem para todas:
//!
//! - **Nada é gravado direto no lugar.** O conteúdo é montado em `.inputremote/montagem`, na mesma
//!   pasta — e portanto no mesmo volume —, e só o `rename` final o põe no caminho: quem abre o
//!   arquivo no meio vê o antigo ou o novo, nunca a metade.
//! - **Nada é apagado.** O que a sincronia tira vai para uma lixeira com a data, e uma faxina tira
//!   o que passou de 30 dias.
//! - **O caminho relativo nunca sai da raiz**: ele já passou por `is_safe_relative_path` no
//!   protocolo, e aqui só é montado componente por componente.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use ir_pasta::ignorar::PASTA_DE_CONTROLE;

/// Quanto tempo o que foi para a lixeira fica lá.
const RETENCAO: Duration = Duration::from_secs(30 * 24 * 3_600);

/// O caminho absoluto de um caminho relativo da pasta.
#[must_use]
pub fn absoluto(raiz: &Path, relativo: &str) -> PathBuf {
    relativo
        .split('/')
        .fold(raiz.to_path_buf(), |caminho, parte| caminho.join(parte))
}

/// Onde o conteúdo em trânsito é montado, nesta pasta.
#[must_use]
pub fn montagem(raiz: &Path) -> PathBuf {
    raiz.join(PASTA_DE_CONTROLE).join("montagem")
}

/// Um arquivo de montagem novo, com este nome.
///
/// # Errors
///
/// Erro de disco ao criar a pasta de montagem.
pub fn arquivo_de_montagem(raiz: &Path, nome: &str) -> std::io::Result<PathBuf> {
    let pasta = montagem(raiz);
    std::fs::create_dir_all(&pasta)?;
    esconder(&raiz.join(PASTA_DE_CONTROLE));
    Ok(pasta.join(nome))
}

/// Põe um arquivo montado no caminho, com o horário dado.
///
/// # Errors
///
/// Erro de disco.
pub fn publicar(montado: &Path, destino: &Path, modificado_ns: i64) -> std::io::Result<()> {
    if let Some(pai) = destino.parent() {
        std::fs::create_dir_all(pai)?;
    }
    ajustar_horario(montado, modificado_ns);
    std::fs::rename(montado, destino)
}

/// Renomeia dentro da pasta.
///
/// # Errors
///
/// Erro de disco.
pub fn mover(de: &Path, para: &Path) -> std::io::Result<()> {
    if let Some(pai) = para.parent() {
        std::fs::create_dir_all(pai)?;
    }
    std::fs::rename(de, para)
}

/// Copia dentro da pasta, montando ao lado antes de pôr no lugar, e mantém o horário da fonte.
///
/// # Errors
///
/// Erro de disco.
pub fn copiar(raiz: &Path, de: &Path, para: &Path) -> std::io::Result<()> {
    let montado = arquivo_de_montagem(raiz, &format!("copia-{}", nanos_agora()))?;
    std::fs::copy(de, &montado)?;
    let horario = std::fs::metadata(de)
        .ok()
        .map_or(0, |dados| crate::varredura::horario(&dados));
    publicar(&montado, para, horario)
}

/// Leva um caminho — arquivo ou subpasta inteira — para a lixeira, numa pasta com a data de hoje.
///
/// # Errors
///
/// Erro de disco. Um caminho que já não existe não é erro.
pub fn para_lixeira(lixeira: &Path, raiz: &Path, relativo: &str) -> std::io::Result<()> {
    let origem = absoluto(raiz, relativo);
    if std::fs::symlink_metadata(&origem).is_err() {
        return Ok(());
    }
    let destino = absoluto(&lixeira.join(dia_de_hoje()), relativo);
    let destino = livre(&destino);
    mover(&origem, &destino)
}

/// Tira da lixeira o que passou da retenção.
pub fn faxina(lixeira: &Path) {
    let Ok(dias) = std::fs::read_dir(lixeira) else {
        return;
    };
    let agora = SystemTime::now();
    for dia in dias.filter_map(Result::ok) {
        let velho = dia
            .metadata()
            .and_then(|dados| dados.modified())
            .ok()
            .and_then(|quando| agora.duration_since(quando).ok())
            .is_some_and(|idade| idade > RETENCAO);
        if velho {
            let _ = std::fs::remove_dir_all(dia.path());
        }
    }
}

/// Um caminho livre perto de `desejado`: ele mesmo, ou com um número no fim.
fn livre(desejado: &Path) -> PathBuf {
    if std::fs::symlink_metadata(desejado).is_err() {
        return desejado.to_path_buf();
    }
    (2..u32::MAX)
        .map(|n| {
            let mut nome = desejado.as_os_str().to_owned();
            nome.push(format!(" ({n})"));
            PathBuf::from(nome)
        })
        .find(|candidato| std::fs::symlink_metadata(candidato).is_err())
        .unwrap_or_else(|| desejado.to_path_buf())
}

/// Ajusta o horário de modificação, para o arquivo daqui mostrar a mesma data do de lá.
pub fn ajustar_horario(caminho: &Path, modificado_ns: i64) {
    let Ok(nanos) = u64::try_from(modificado_ns) else {
        return;
    };
    let quando = UNIX_EPOCH + Duration::from_nanos(nanos);
    if let Ok(arquivo) = std::fs::OpenOptions::new().write(true).open(caminho) {
        let _ = arquivo.set_modified(quando);
    }
}

/// Agora, em nanossegundos desde 1970.
#[must_use]
pub fn nanos_agora() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|desde| i64::try_from(desde.as_nanos()).ok())
        .unwrap_or(0)
}

/// O dia de hoje, `2026-10-02`, em UTC: o nome da pasta do dia na lixeira.
fn dia_de_hoje() -> String {
    let momento = ir_pasta::conflito::Momento::de_unix_ns(nanos_agora(), 0);
    format!("{:04}-{:02}-{:02}", momento.ano, momento.mes, momento.dia)
}

/// A pasta de controle não aparece no Explorer: é da sincronia, não do usuário.
#[cfg(windows)]
fn esconder(caminho: &Path) {
    use std::os::windows::process::CommandExt;
    const SEM_JANELA: u32 = 0x0800_0000;
    let _ = std::process::Command::new("attrib")
        .arg("+h")
        .arg(caminho)
        .creation_flags(SEM_JANELA)
        .status();
}

/// No Linux o ponto no começo do nome já a esconde.
#[cfg(not(windows))]
const fn esconder(_caminho: &Path) {}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn a_lixeira_guarda_com_a_data_e_nao_sobrescreve() {
        let raiz = std::env::temp_dir().join(format!("ir-disco-{}", std::process::id()));
        let lixeira = raiz.join(".inputremote").join("lixeira");
        let _ = std::fs::remove_dir_all(&raiz);
        std::fs::create_dir_all(raiz.join("d")).unwrap();
        std::fs::write(raiz.join("d/a.txt"), b"1").unwrap();
        para_lixeira(&lixeira, &raiz, "d/a.txt").unwrap();
        std::fs::write(raiz.join("d/a.txt"), b"2").unwrap();
        para_lixeira(&lixeira, &raiz, "d/a.txt").unwrap();
        let dia = lixeira.join(dia_de_hoje()).join("d");
        assert_eq!(std::fs::read(dia.join("a.txt")).unwrap(), b"1");
        assert_eq!(std::fs::read(dia.join("a.txt (2)")).unwrap(), b"2");
        assert!(para_lixeira(&lixeira, &raiz, "nao-existe").is_ok());
        let _ = std::fs::remove_dir_all(&raiz);
    }

    #[test]
    fn publicar_cria_as_pastas_e_poe_o_horario() {
        let raiz = std::env::temp_dir().join(format!("ir-publicar-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&raiz);
        let montado = arquivo_de_montagem(&raiz, "x").unwrap();
        std::fs::write(&montado, b"conteudo").unwrap();
        let destino = absoluto(&raiz, "a/b/c.txt");
        let horario = 1_700_000_000_000_000_000;
        publicar(&montado, &destino, horario).unwrap();
        let dados = std::fs::metadata(&destino).unwrap();
        assert_eq!(crate::varredura::horario(&dados), horario);
        let _ = std::fs::remove_dir_all(&raiz);
    }
}
