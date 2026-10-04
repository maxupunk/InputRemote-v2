//! O atalho da pasta recebida na barra lateral do gerenciador de arquivos.
//!
//! No Linux, o arquivo de favoritos do GTK (`~/.config/gtk-3.0/bookmarks`), que o Nautilus e os
//! seletores de arquivo leem: uma linha `file:///caminho Nome` por atalho. Reescrito inteiro e por
//! troca atômica, para um Nautilus lendo no meio nunca ver o arquivo pela metade.
//!
//! No Windows a pasta aparece no painel do Explorer pela raiz de sincronia (a fase sob demanda);
//! até lá, ela está em `%USERPROFILE%\InputRemote`, ao lado das pastas do usuário.

use std::path::Path;

/// Conta na tela, fora da janela, que o outro computador quer compartilhar uma pasta.
///
/// No Linux pelo `notify-send`, como o ajudante de clipboard conta as cópias; no Windows quem
/// avisa é a janela, no canto da tela, quando a lista de pastas muda.
pub fn avisar_oferta(par: &str, nome: &str) {
    #[cfg(target_os = "linux")]
    ir_recado::linux::avisar(&ir_recado::Recado::oferta_de_pasta(par, nome));
    #[cfg(not(target_os = "linux"))]
    let _ = (par, nome);
}

/// Põe o atalho, se ainda não houver.
pub fn por(raiz: &Path) {
    #[cfg(not(windows))]
    linux::mexer(|linhas| {
        let linha = linux::linha(raiz);
        if !linhas.contains(&linha) {
            linhas.push(linha);
        }
    });
    #[cfg(windows)]
    let _ = raiz;
}

/// Tira o atalho.
pub fn tirar(raiz: &Path) {
    #[cfg(not(windows))]
    linux::mexer(|linhas| {
        let prefixo = linux::url(raiz);
        linhas.retain(|l| l.split(' ').next() != Some(prefixo.as_str()));
    });
    #[cfg(windows)]
    let _ = raiz;
}

#[cfg(not(windows))]
mod linux {
    use std::path::{Path, PathBuf};

    fn arquivo() -> Option<PathBuf> {
        let base = std::env::var_os("XDG_CONFIG_HOME")
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|casa| Path::new(&casa).join(".config")))?;
        Some(base.join("gtk-3.0").join("bookmarks"))
    }

    pub(super) fn mexer(mudar: impl FnOnce(&mut Vec<String>)) {
        let Some(arquivo) = arquivo() else { return };
        let texto = std::fs::read_to_string(&arquivo).unwrap_or_default();
        let mut linhas: Vec<String> = texto.lines().map(str::to_owned).collect();
        let antes = linhas.clone();
        mudar(&mut linhas);
        if linhas == antes {
            return;
        }
        if let Some(pai) = arquivo.parent() {
            let _ = std::fs::create_dir_all(pai);
        }
        let novo = arquivo.with_extension("inputremote-novo");
        let mut conteudo = linhas.join("\n");
        conteudo.push('\n');
        if std::fs::write(&novo, conteudo).is_ok() {
            let _ = std::fs::rename(&novo, &arquivo);
        }
    }

    pub(super) fn linha(raiz: &Path) -> String {
        let nome = raiz
            .file_name()
            .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
        format!("{} {nome}", url(raiz))
    }

    /// O caminho como URL `file://`, com o que não é seguro em URL codificado.
    pub(super) fn url(raiz: &Path) -> String {
        use std::fmt::Write;
        let mut url = String::from("file://");
        for byte in raiz.to_string_lossy().bytes() {
            if byte.is_ascii_alphanumeric() || b"/-_.~".contains(&byte) {
                url.push(char::from(byte));
            } else {
                let _ = write!(url, "%{byte:02X}");
            }
        }
        url
    }

    #[cfg(test)]
    mod testes {
        use super::*;

        #[test]
        fn espaco_e_acento_viram_codigo_na_url() {
            let raiz = Path::new("/home/ana/InputRemote/Relatório 2026");
            assert_eq!(
                linha(raiz),
                "file:///home/ana/InputRemote/Relat%C3%B3rio%202026 Relatório 2026"
            );
        }
    }
}
