//! A imagem do clipboard atravessa como arquivo.
//!
//! Uma captura de tela passa fácil de alguns megabytes, e o canal 4 da sessão leva texto curto.
//! Quem leva volume é o canal de dados, que já existe para os arquivos, com retomada, prazo e
//! resumo. Então a imagem vira um arquivo PNG com um nome que a denuncia, vai pelo mesmo caminho de
//! uma cópia de arquivo, e do outro lado o ajudante reconhece o nome e publica **imagem**, e não um
//! arquivo — quem copiou uma imagem quer colar uma imagem.
//!
//! O arquivo é passagem, dos dois lados: aqui ele mora numa pasta temporária que só guarda o último;
//! lá, é apagado depois de publicado.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use ir_clip::Conteudo;
use tracing::debug;

/// O começo do nome que denuncia uma imagem de clipboard.
const PREFIXO: &str = "inputremote-imagem-";

/// A pasta, dentro da temporária do usuário, onde a imagem espera ser enviada.
const PASTA: &str = "InputRemote-imagens";

/// Onde a imagem espera: uma pasta que só o usuário abre.
///
/// No Linux, `XDG_RUNTIME_DIR` (`/run/user/<uid>`, `0700`, em memória), que o serviço ainda lê —
/// `ProtectHome=read-only` o deixa ler, não escrever. No Windows, a temporária do usuário, que já é
/// dele só.
pub(super) fn pasta_temporaria() -> PathBuf {
    #[cfg(not(windows))]
    if let Some(pasta) = std::env::var_os("XDG_RUNTIME_DIR") {
        return PathBuf::from(pasta);
    }
    std::env::temp_dir()
}

/// Grava a imagem para enviar, e diz onde.
///
/// O nome leva parte do resumo: duas imagens diferentes nunca se confundem na pasta do par, e a
/// mesma imagem copiada de novo tem o mesmo nome. As anteriores são apagadas — só a última pode
/// estar em trânsito, porque o clipboard só tem uma.
///
/// # Errors
///
/// Os da escrita no disco.
pub(super) fn gravar_para_enviar(png: &[u8], pasta_temporaria: &Path) -> io::Result<PathBuf> {
    let pasta = pasta_temporaria.join(PASTA);
    fs::create_dir_all(&pasta)?;
    let resumo = blake3::hash(png).to_hex();
    let nome = format!(
        "{PREFIXO}{}.png",
        resumo.as_str().get(..16).unwrap_or_default()
    );
    if let Ok(entradas) = fs::read_dir(&pasta) {
        for entrada in entradas.flatten() {
            if entrada.file_name().to_string_lossy() != nome {
                let _ = fs::remove_file(entrada.path());
            }
        }
    }
    let caminho = pasta.join(nome);
    fs::write(&caminho, png)?;
    Ok(caminho)
}

/// Se o que chegou é uma imagem de clipboard, e não um arquivo que o usuário copiou.
///
/// Pelo nome, com a folga do sufixo que a pasta de recebidos põe quando o nome já existe
/// (`… (2).png`).
pub(super) fn e_imagem_do_clipboard(caminho: &Path) -> bool {
    caminho.file_name().is_some_and(|nome| {
        let nome = nome.to_string_lossy();
        nome.starts_with(PREFIXO)
            && Path::new(nome.as_ref())
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("png"))
    }) && caminho.is_file()
}

/// Lê a imagem que chegou, para publicar; o arquivo é apagado quando já está lida.
///
/// # Errors
///
/// Os da leitura.
pub(super) fn ler_recebida(caminho: &Path) -> io::Result<Conteudo> {
    let png = fs::read(caminho)?;
    if let Err(erro) = fs::remove_file(caminho) {
        // Sobra um arquivo na pasta de recebidos, que a limpeza dela leva depois. Não impede colar.
        debug!(%erro, "não consegui apagar a imagem recebida");
    }
    Ok(Conteudo::Imagem(png))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    fn pasta_de_teste(nome: &str) -> PathBuf {
        let pasta = std::env::temp_dir().join(format!("ir-imagem-{nome}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&pasta);
        fs::create_dir_all(&pasta).unwrap();
        pasta
    }

    #[test]
    fn a_imagem_gravada_e_reconhecida_do_outro_lado() {
        let temporaria = pasta_de_teste("ida");
        let caminho = gravar_para_enviar(b"png de mentira", &temporaria).unwrap();
        assert!(e_imagem_do_clipboard(&caminho));
        assert_eq!(
            ler_recebida(&caminho).unwrap(),
            Conteudo::Imagem(b"png de mentira".to_vec())
        );
        assert!(!caminho.exists(), "a imagem recebida é passagem");
        let _ = fs::remove_dir_all(temporaria);
    }

    #[test]
    fn so_a_ultima_imagem_fica_esperando() {
        let temporaria = pasta_de_teste("ultima");
        let primeira = gravar_para_enviar(b"um", &temporaria).unwrap();
        let segunda = gravar_para_enviar(b"dois", &temporaria).unwrap();
        assert_ne!(primeira, segunda);
        assert!(!primeira.exists());
        assert!(segunda.exists());
        // A mesma imagem de novo tem o mesmo nome, e não apaga a si mesma.
        assert_eq!(gravar_para_enviar(b"dois", &temporaria).unwrap(), segunda);
        assert!(segunda.exists());
        let _ = fs::remove_dir_all(temporaria);
    }

    #[test]
    fn um_arquivo_comum_nao_vira_imagem() {
        let temporaria = pasta_de_teste("comum");
        let foto = temporaria.join("ferias.png");
        fs::write(&foto, b"x").unwrap();
        assert!(!e_imagem_do_clipboard(&foto));
        let renomeada = temporaria.join("inputremote-imagem-0123 (2).png");
        fs::write(&renomeada, b"x").unwrap();
        assert!(
            e_imagem_do_clipboard(&renomeada),
            "o sufixo de colisão ainda é imagem"
        );
        assert!(
            !e_imagem_do_clipboard(&temporaria.join("inputremote-imagem-x.png")),
            "não existe"
        );
        let pasta = temporaria.join("inputremote-imagem-pasta.png");
        fs::create_dir_all(&pasta).unwrap();
        assert!(
            !e_imagem_do_clipboard(&pasta),
            "uma pasta com esse nome não é imagem"
        );
        let _ = fs::remove_dir_all(temporaria);
    }
}
