//! A versão do protocolo que o par negociou na última sessão, guardada em disco com a chave dele.
//!
//! É o que deixa as pastas andarem logo depois de o serviço subir, antes de a sessão de entrada
//! voltar — e mesmo se ela não voltar: as pastas não dependem do teclado e do mouse. Com a chave
//! junto, um par novo nunca herda a versão do anterior.
//!
//! Saiu do `ir-daemon`, onde morava ao lado de quem sobe o canal de arquivos: é o que a máquina
//! guarda em disco, e isso é deste crate.

use std::path::Path;

use ir_proto::version::ProtocolVersion;

/// O arquivo, na pasta de estado.
const ARQUIVO: &str = "versao-do-par";

/// A versão que o par negociou da última vez, se a chave guardada é a dele.
#[must_use]
pub fn guardada(dir: &Path, chave: &str) -> Option<ProtocolVersion> {
    let texto = std::fs::read_to_string(dir.join(ARQUIVO)).ok()?;
    let (de_quem, versao) = texto.trim().split_once(' ')?;
    (de_quem == chave)
        .then(|| versao.parse().ok().map(ProtocolVersion))
        .flatten()
}

/// Guarda a versão que o par negociou agora. Falhar só custa esperar a sessão na próxima subida.
pub fn guardar(dir: &Path, chave: &str, versao: ProtocolVersion) {
    let _ = std::fs::write(dir.join(ARQUIVO), format!("{chave} {}", versao.get()));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pasta(nome: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("ir-versao-{nome}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_versao_volta_para_o_mesmo_par_e_nao_para_outro() {
        let dir = pasta("par");
        assert_eq!(guardada(&dir, "abc"), None, "nada guardado ainda");
        guardar(&dir, "abc", ProtocolVersion(11));
        assert_eq!(guardada(&dir, "abc"), Some(ProtocolVersion(11)));
        assert_eq!(guardada(&dir, "outro"), None, "um par novo não herda");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn um_arquivo_estragado_vale_nada() {
        let dir = pasta("lixo");
        std::fs::write(dir.join(ARQUIVO), "abc onze").unwrap();
        assert_eq!(guardada(&dir, "abc"), None);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
