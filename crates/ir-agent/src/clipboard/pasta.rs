//! A cópia (Ctrl+C) e a pasta compartilhada.
//!
//! Duas regras, para o mesmo conteúdo não atravessar a rede duas vezes ao mesmo tempo:
//!
//! - **Copiar de dentro de uma pasta compartilhada não manda bytes.** O arquivo já está nos dois
//!   computadores; vão os caminhos, pela pasta, e o outro computador põe no clipboard dele os mesmos
//!   arquivos da cópia dele ([`Pastas::separar`]).
//! - **O que atravessou pela cópia fica anotado** ([`anotar`]): o que foi daqui e o que chegou. Se a
//!   pessoa colar na pasta compartilhada, a sincronia acha o conteúdo neste computador pelo resumo e
//!   não o pede de novo à rede (`ir_acervo::conhecidos`).

use std::ffi::OsStr;
use std::path::{Component, Path, PathBuf};

use ir_clip::Conteudo;
use ir_ipc::Pedido;
use ir_ipc::pastas::{ComandoDePasta, IdDePasta, ResumoDePasta, SituacaoDaPasta};
use tracing::debug;

/// As pastas compartilhadas deste computador: o identificador e onde a pessoa as vê.
#[derive(Debug, Default)]
pub(super) struct Pastas(Vec<(IdDePasta, PathBuf)>);

impl Pastas {
    /// Uma lista com uma pasta só, para os testes do laço.
    #[cfg(test)]
    pub(super) fn com(pasta: IdDePasta, raiz: PathBuf) -> Self {
        Self(vec![(pasta, raiz)])
    }

    /// A lista nova, do serviço. Oferta ainda não aceita não conta: não tem pasta aqui.
    pub(super) fn atualizar(&mut self, lista: &[ResumoDePasta]) {
        self.0 = lista
            .iter()
            .filter(|p| p.situacao != SituacaoDaPasta::Oferecida && !p.caminho_local.is_empty())
            .map(|p| (p.id, PathBuf::from(&p.caminho_local)))
            .collect();
    }

    /// Separa uma cópia de arquivos: o pedido da pasta, quando todos são de uma pasta só; e os de
    /// fora, que seguem pela cópia de arquivos. Os de dentro de uma pasta nunca vão pela cópia —
    /// numa cópia misturada, o clipboard do outro lado recebe só os de fora.
    pub(super) fn separar(&self, caminhos: &[PathBuf]) -> (Option<Pedido>, Vec<PathBuf>) {
        let mut da_pasta: Option<(IdDePasta, Vec<String>)> = None;
        let mut fora = Vec::new();
        for caminho in caminhos {
            match self.dentro(caminho) {
                Some((pasta, relativo)) => match &mut da_pasta {
                    None => da_pasta = Some((pasta, vec![relativo])),
                    Some((primeira, lista)) if *primeira == pasta => lista.push(relativo),
                    Some(_) => {}
                },
                None => fora.push(caminho.clone()),
            }
        }
        let pedido = da_pasta
            .filter(|_| fora.is_empty())
            .map(|(pasta, caminhos)| Pedido::Pasta(ComandoDePasta::Copiado { pasta, caminhos }));
        (pedido, fora)
    }

    /// O pedido que leva este conteúdo, olhando as pastas: o que é de dentro de uma pasta não vai
    /// pela cópia de arquivos. `para` faz o pedido de sempre, do que sobra.
    pub(super) fn pedido(
        &self,
        conteudo: &Conteudo,
        para: fn(&Conteudo) -> Option<Pedido>,
    ) -> Option<Pedido> {
        let Conteudo::Arquivos(caminhos) = conteudo else {
            return para(conteudo);
        };
        let (da_pasta, fora) = self.separar(caminhos);
        if da_pasta.is_some() {
            return da_pasta;
        }
        (!fora.is_empty())
            .then(|| para(&Conteudo::Arquivos(fora)))
            .flatten()
    }

    fn dentro(&self, caminho: &Path) -> Option<(IdDePasta, String)> {
        self.0
            .iter()
            .find_map(|(pasta, raiz)| relativo(raiz, caminho).map(|r| (*pasta, r)))
    }
}

/// O caminho relativo a `raiz`, com `/`, quando ele está dentro dela (e não é ela mesma).
fn relativo(raiz: &Path, caminho: &Path) -> Option<String> {
    let mut partes = caminho.components();
    for parte in raiz.components() {
        if !igual(parte, partes.next()?) {
            return None;
        }
    }
    let resto: Vec<&str> = partes
        .map(|parte| match parte {
            Component::Normal(nome) => nome.to_str(),
            _ => None,
        })
        .collect::<Option<_>>()?;
    (!resto.is_empty()).then(|| resto.join("/"))
}

/// No Windows, `C:\Users\Ana` e `c:\users\ana` são a mesma pasta.
fn igual(a: Component<'_>, b: Component<'_>) -> bool {
    let (a, b): (&OsStr, &OsStr) = (a.as_os_str(), b.as_os_str());
    if cfg!(windows) {
        a.to_string_lossy().to_lowercase() == b.to_string_lossy().to_lowercase()
    } else {
        a == b
    }
}

/// Anota o que atravessou pela cópia — o que foi daqui, o que chegou —, para a pasta compartilhada
/// achá-lo aqui em vez de pedir à rede.
pub(super) fn anotar(caminhos: &[PathBuf]) {
    match ir_sincronia::Lugar::deste_usuario() {
        Ok(lugar) => ir_sincronia::conhecidos::registrar(&lugar.estado, caminhos),
        Err(erro) => debug!(%erro, "sem pasta de estado para anotar o que foi copiado"),
    }
}

#[cfg(test)]
#[allow(clippy::panic, clippy::unwrap_used, clippy::expect_used)]
mod testes {
    use super::*;

    fn pastas() -> Pastas {
        let raiz = if cfg!(windows) {
            PathBuf::from(r"C:\Users\Ana\InputRemote\Temp")
        } else {
            PathBuf::from("/home/ana/InputRemote/Temp")
        };
        Pastas(vec![(IdDePasta([7; 16]), raiz)])
    }

    fn em(relativo: &str) -> PathBuf {
        let base = if cfg!(windows) {
            r"c:\users\ana\inputremote\temp"
        } else {
            "/home/ana/InputRemote/Temp"
        };
        relativo
            .split('/')
            .fold(PathBuf::from(base), |c, p| c.join(p))
    }

    #[test]
    fn copiar_de_dentro_da_pasta_vira_caminhos_da_pasta_e_nao_bytes() {
        let (pedido, fora) = pastas().separar(&[em("fotos/img2.jpg"), em("notas.txt")]);
        assert!(fora.is_empty());
        let Some(Pedido::Pasta(ComandoDePasta::Copiado { pasta, caminhos })) = pedido else {
            panic!("esperava o pedido da pasta: {pedido:?}")
        };
        assert_eq!(pasta, IdDePasta([7; 16]));
        assert_eq!(caminhos, vec!["fotos/img2.jpg", "notas.txt"]);
    }

    #[test]
    fn os_de_fora_seguem_pela_copia_e_os_de_dentro_nunca() {
        let fora_da_pasta = PathBuf::from(if cfg!(windows) {
            r"C:\Fotos\a.jpg"
        } else {
            "/tmp/a.jpg"
        });
        let (pedido, fora) = pastas().separar(&[em("dentro.txt"), fora_da_pasta.clone()]);
        assert!(pedido.is_none());
        assert_eq!(fora, vec![fora_da_pasta]);
        // A própria pasta, e uma pasta vizinha de nome parecido, não são "de dentro".
        let vizinha = PathBuf::from(if cfg!(windows) {
            r"C:\Users\Ana\InputRemote\Temp2\x.txt"
        } else {
            "/home/ana/InputRemote/Temp2/x.txt"
        });
        let (pedido, fora) = pastas().separar(std::slice::from_ref(&vizinha));
        assert!(pedido.is_none());
        assert_eq!(fora, vec![vizinha]);
    }
}
