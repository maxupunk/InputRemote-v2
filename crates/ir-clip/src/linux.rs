//! O backend do Linux: `wl-clipboard`, sobre o que foi **medido** no GNOME 50 da bancada.
//!
//! | Operação | GNOME 50 | Como |
//! |---|---|---|
//! | publicar texto, imagem PNG ou lista de arquivos | funciona | `wl-copy`, que fica vivo como dono da seleção |
//! | ler | funciona | `wl-paste`, com prazo |
//! | ser avisado de mudança | **não existe** | `wl-paste --watch` exige `data-control`, e o GNOME não o expõe |
//!
//! A terceira linha é o motivo do [ADR-0011](../../../docs/adr/0011-clipboard-na-travessia.md): sem
//! aviso, o clipboard é lido quando o controle sai desta máquina. O aviso ainda é tentado — KDE e
//! Sway expõem `data-control` —, e a ausência é **descoberta na hora**, nunca presumida
//! ([06, §6](../../../docs/06-linux.md)).
//!
//! # Por que `wl-clipboard` e não o protocolo direto
//!
//! Falar Wayland daqui exigiria uma biblioteca de cliente Wayland inteira e uma superfície própria,
//! para fazer o que o `wl-clipboard` já faz e que as distribuições já empacotam. A dependência vira
//! de pacote (`Requires: wl-clipboard`), e não de compilação.
//!
//! # Dois fatos medidos que o código precisa respeitar
//!
//! **`wl-copy` não termina.** Ele é o dono da seleção enquanto viver: é ele que responde quando
//! alguém cola. Esperar que ele saia trava; matá-lo apaga o clipboard. Ele sai sozinho quando outro
//! programa toma a seleção. Então é lançado, alimentado e **deixado vivo**, e os que já saíram são
//! recolhidos na publicação seguinte.
//!
//! **Fora da sessão gráfica ele não serve.** Rodado por SSH, `wl-paste` travava; rodado dentro do
//! gerenciador de serviços do usuário, funcionou na primeira. O ajudante roda na sessão, e o prazo
//! em toda leitura é o que impede um compositor mudo de travá-lo.

use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdout, Command, Stdio};
use std::thread::sleep;
use std::time::Duration;

use crate::conteudo::Conteudo;
use crate::error::{ClipError, Result};
use crate::uri;
use crate::{Clipboard, Vigia};

/// O prazo de cada leitura, em segundos, passado ao `timeout` do coreutils.
///
/// Uma leitura depende do **dono** da seleção responder — é outro programa. Se ele travou, esperar
/// mais não o destrava.
const PRAZO: &str = "3";

/// O código de saída do `timeout` quando o prazo venceu.
const PRAZO_VENCIDO: i32 = 124;

/// O clipboard da sessão Wayland.
#[derive(Debug, Default)]
pub struct ClipboardDoWayland {
    /// Os `wl-copy` lançados. Só o último é dono da seleção; os outros já saíram ou vão sair.
    donos: Vec<Child>,
}

impl ClipboardDoWayland {
    /// Recolhe os `wl-copy` que já saíram, para não acumular processo morto.
    fn recolher(&mut self) {
        self.donos
            .retain_mut(|filho| matches!(filho.try_wait(), Ok(None)));
    }
}

impl Clipboard for ClipboardDoWayland {
    fn ler(&mut self) -> Result<Option<Conteudo>> {
        let Some(tipos) = colar(&["--list-types"])? else {
            return Ok(None); // clipboard vazio
        };
        let tipos = String::from_utf8_lossy(&tipos);
        let tem = |procurado: &str| tipos.lines().any(|tipo| tipo.trim() == procurado);

        // Arquivos antes de texto, pelo mesmo motivo do Windows: o gerenciador de arquivos oferece
        // os dois, e olhar texto primeiro transformaria "copiei um arquivo" em "copiei um nome".
        if tem("text/uri-list") {
            let bytes = colar(&["--no-newline", "--type", "text/uri-list"])?.unwrap_or_default();
            return Ok(Some(arquivos(&uri::lista(&String::from_utf8_lossy(
                &bytes,
            )))));
        }
        if tem("x-special/gnome-copied-files") {
            // A primeira linha é `copy` ou `cut`; as outras, URIs.
            let bytes = colar(&["--no-newline", "--type", "x-special/gnome-copied-files"])?
                .unwrap_or_default();
            let texto = String::from_utf8_lossy(&bytes);
            let resto: String = texto.lines().skip(1).collect::<Vec<_>>().join("\n");
            return Ok(Some(arquivos(&uri::lista(&resto))));
        }
        let texto_disponivel = tipos
            .lines()
            .any(|tipo| tipo.starts_with("text/plain") || tipo.trim() == "UTF8_STRING");
        if texto_disponivel {
            let bytes = colar(&["--no-newline"])?.unwrap_or_default();
            return Ok(Some(Conteudo::texto(&String::from_utf8_lossy(&bytes))));
        }
        // Imagem depois de texto, como no Windows: a planilha oferece uma figura das células junto
        // com o texto delas. E só PNG, a forma canônica — que é a que todo compositor oferece.
        if tem("image/png") {
            let bytes = colar(&["--type", "image/png"])?.unwrap_or_default();
            return Ok((!bytes.is_empty()).then_some(Conteudo::Imagem(bytes)));
        }
        // Formato de algum aplicativo que o protocolo não transporta. Não é erro.
        Ok(None)
    }

    fn publicar(&mut self, conteudo: &Conteudo) -> Result<()> {
        let (tipo, dados) = match conteudo {
            Conteudo::Texto(texto) => ("text/plain;charset=utf-8", texto.clone().into_bytes()),
            Conteudo::Arquivos(caminhos) => {
                let textos: Vec<String> = caminhos
                    .iter()
                    .map(|caminho| caminho.to_string_lossy().into_owned())
                    .collect();
                ("text/uri-list", uri::montar_lista(&textos).into_bytes())
            }
            // PNG no protocolo e PNG no Wayland: nada a converter.
            Conteudo::Imagem(png) => ("image/png", png.clone()),
        };
        self.recolher();
        let mut filho = Command::new("wl-copy")
            .args(["--type", tipo])
            .stdin(Stdio::piped())
            // O `wl-copy` fica vivo: herdar a saída prenderia quem nos lançou — foi exatamente o
            // que travou o SSH da bancada.
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|erro| ausente_ou_falha(&erro))?;
        if let Some(mut entrada) = filho.stdin.take() {
            entrada
                .write_all(&dados)
                .map_err(|erro| ClipError::Sistema(erro.to_string()))?;
            // Fechar a entrada é o que diz ao `wl-copy` que o conteúdo acabou.
        }
        self.donos.push(filho);
        Ok(())
    }
}

/// Uma lista de arquivos, a partir de caminhos já decodificados.
fn arquivos(caminhos: &[String]) -> Conteudo {
    Conteudo::Arquivos(caminhos.iter().map(PathBuf::from).collect())
}

/// Roda `wl-paste` com prazo. `None` quando o clipboard está vazio ou sem o tipo pedido.
fn colar(argumentos: &[&str]) -> Result<Option<Vec<u8>>> {
    let saida = Command::new("timeout")
        .arg(PRAZO)
        .arg("wl-paste")
        .args(argumentos)
        .stdin(Stdio::null())
        .output()
        .map_err(|erro| ausente_ou_falha(&erro))?;
    match saida.status.code() {
        Some(0) => Ok(Some(saida.stdout)),
        Some(PRAZO_VENCIDO) => Err(ClipError::Sistema(
            "o programa dono do clipboard não respondeu".to_owned(),
        )),
        // `wl-paste` sai com 1 para "nada copiado" e para "não há esse tipo". Os dois são ausência
        // de conteúdo, e não defeito.
        _ => Ok(None),
    }
}

/// Programa ausente é ausência declarada; o resto é falha.
fn ausente_ou_falha(erro: &std::io::Error) -> ClipError {
    if erro.kind() == std::io::ErrorKind::NotFound {
        ClipError::Indisponivel("o wl-clipboard não está instalado")
    } else {
        ClipError::Sistema(erro.to_string())
    }
}

/// Abre o clipboard desta sessão.
///
/// # Errors
///
/// [`ClipError::Indisponivel`] fora de uma sessão Wayland — um *greeter*, um console, um SSH.
pub fn abrir() -> Result<Box<dyn Clipboard>> {
    if std::env::var_os("WAYLAND_DISPLAY").is_none() {
        return Err(ClipError::Indisponivel(
            "não há sessão Wayland neste processo",
        ));
    }
    Ok(Box::new(ClipboardDoWayland::default()))
}

/// O aviso de mudança, quando o compositor o oferece.
#[derive(Debug)]
struct VigiaDoWayland {
    filho: Child,
    linhas: BufReader<ChildStdout>,
}

impl Vigia for VigiaDoWayland {
    fn proxima(&mut self) -> Result<()> {
        let mut linha = String::new();
        match self.linhas.read_line(&mut linha) {
            Ok(0) | Err(_) => Err(ClipError::Indisponivel("o aviso de clipboard encerrou")),
            Ok(_) => Ok(()),
        }
    }
}

impl Drop for VigiaDoWayland {
    fn drop(&mut self) {
        let _ = self.filho.kill();
        let _ = self.filho.wait();
    }
}

/// Começa a vigiar, se o compositor souber avisar.
///
/// # Errors
///
/// [`ClipError::Indisponivel`] onde não há `data-control` — o GNOME, medido. Não é defeito: aí a
/// sincronização acontece na travessia ([ADR-0011](../../../docs/adr/0011-clipboard-na-travessia.md)).
pub fn vigiar() -> Result<Box<dyn Vigia>> {
    if std::env::var_os("WAYLAND_DISPLAY").is_none() {
        return Err(ClipError::Indisponivel(
            "não há sessão Wayland neste processo",
        ));
    }
    // `echo` a cada mudança; uma linha por aviso.
    let mut filho = Command::new("wl-paste")
        .args(["--watch", "echo", "mudou"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|erro| ausente_ou_falha(&erro))?;
    // Sem `data-control`, ele sai na hora com a queixa. Um instante basta para saber.
    sleep(Duration::from_millis(300));
    if matches!(filho.try_wait(), Ok(Some(_))) {
        return Err(ClipError::Indisponivel(
            "este compositor não avisa mudança de clipboard (sem data-control)",
        ));
    }
    let Some(saida) = filho.stdout.take() else {
        let _ = filho.kill();
        return Err(ClipError::Sistema("o aviso não abriu a saída".to_owned()));
    };
    let mut linhas = BufReader::new(saida);
    // `--watch` avisa uma vez ao começar, com o conteúdo que já estava lá. Não é cópia nova do
    // usuário — e no Windows o aviso equivalente não existe. Descartar mantém os dois iguais.
    let mut primeira = String::new();
    let _ = linhas.read_line(&mut primeira);
    Ok(Box::new(VigiaDoWayland { filho, linhas }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fora_de_uma_sessao_wayland_a_ausencia_e_declarada() {
        // Um teste em CI, num console ou por SSH não tem `WAYLAND_DISPLAY`. A resposta tem de ser
        // "não há clipboard aqui", e não um erro, nem um processo pendurado.
        if std::env::var_os("WAYLAND_DISPLAY").is_some() {
            return; // rodando numa sessão de verdade; o caso não se aplica
        }
        let erro = abrir().err().expect("sem sessão, sem clipboard");
        assert!(erro.e_ausencia(), "{erro}");
        let erro = vigiar().err().expect("sem sessão, sem aviso");
        assert!(erro.e_ausencia(), "{erro}");
    }
}
