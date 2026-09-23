//! Arquivos copiados de uma pasta de rede: o ajudante os traz para perto antes de pedir o envio.
//!
//! Quem lê o que vai ser enviado é o serviço, e ele recusa pasta de rede de propósito
//! ([04](../../../docs/04-seguranca.md)): como SYSTEM, abrir `\\servidor\pasta` faria a conta da
//! máquina se autenticar num servidor qualquer, e uma unidade mapeada (`Z:`) nem existe para ele —
//! ela é da sessão do usuário. No Linux, uma pasta de rede aberta pelo gerenciador de arquivos
//! (`gvfs`) é uma montagem que só o usuário lê.
//!
//! O ajudante roda **como o usuário**, com as credenciais de rede dele. Então é ele quem copia para
//! uma pasta local sua, e o serviço envia a cópia — sem ganhar acesso a nada que o usuário já não
//! tivesse. Com um teto: uma pasta de rede de centenas de gigabytes não pode encher o disco daqui
//! em silêncio. Acima dele, o caminho segue como estava, e a recusa do serviço aparece na tela.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use tracing::{info, warn};

/// A pasta, dentro da temporária do usuário, onde as cópias esperam ser enviadas.
const PASTA: &str = "InputRemote-envio";

/// O máximo que se copia de uma pasta de rede para enviar.
pub(super) const TETO: u64 = 8 * 1024 * 1024 * 1024;

/// Depois de quanto tempo uma cópia antiga pode ser apagada: tempo de sobra para o envio dela
/// terminar, mesmo grande.
const VALIDADE: Duration = Duration::from_secs(6 * 60 * 60);

/// Os caminhos a enviar, com os de rede trocados por cópias locais.
pub(super) fn trazer_para_perto(caminhos: &[PathBuf], pasta_temporaria: &Path) -> Vec<PathBuf> {
    if !caminhos.iter().any(|caminho| e_de_rede(caminho)) {
        return caminhos.to_vec();
    }
    let raiz = pasta_temporaria.join(PASTA);
    limpar_antigas(&raiz);
    let destino = raiz.join(carimbo());
    let mut restante = TETO;
    caminhos
        .iter()
        .map(|caminho| {
            if !e_de_rede(caminho) {
                return caminho.clone();
            }
            match copiar_para(caminho, &destino, &mut restante) {
                Ok(copia) => {
                    info!("arquivo de uma pasta de rede copiado para perto, para enviar");
                    copia
                }
                Err(erro) => {
                    // Segue o original: o serviço recusa, e a recusa aparece na tela com o motivo.
                    warn!(%erro, "não consegui trazer para perto o que veio de uma pasta de rede");
                    caminho.clone()
                }
            }
        })
        .collect()
}

/// Se o caminho está numa pasta de rede que o serviço não pode ler.
pub(super) fn e_de_rede(caminho: &Path) -> bool {
    #[cfg(windows)]
    {
        // A unidade mapeada vira `\\?\UNC\...` quando resolvida; o caminho UNC já é.
        e_unc(caminho) || fs::canonicalize(caminho).is_ok_and(|real| e_unc(&real))
    }
    #[cfg(not(windows))]
    {
        caminho
            .components()
            .any(|parte| parte.as_os_str() == "gvfs")
            && caminho.starts_with("/run/user")
    }
}

#[cfg(windows)]
fn e_unc(caminho: &Path) -> bool {
    use std::path::{Component, Prefix};
    matches!(
        caminho.components().next(),
        Some(Component::Prefix(prefixo))
            if matches!(prefixo.kind(), Prefix::UNC(..) | Prefix::VerbatimUNC(..))
    )
}

/// Copia um arquivo ou uma pasta inteira para dentro de `destino`, descontando do que resta.
fn copiar_para(origem: &Path, destino: &Path, restante: &mut u64) -> io::Result<PathBuf> {
    let nome = origem
        .file_name()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "caminho sem nome"))?;
    fs::create_dir_all(destino)?;
    let alvo = destino.join(nome);
    if let Err(erro) = copiar_arvore(origem, &alvo, restante) {
        let _ = fs::remove_dir_all(&alvo);
        let _ = fs::remove_file(&alvo);
        return Err(erro);
    }
    Ok(alvo)
}

fn copiar_arvore(origem: &Path, alvo: &Path, restante: &mut u64) -> io::Result<()> {
    let tipo = fs::symlink_metadata(origem)?;
    if tipo.is_dir() {
        fs::create_dir_all(alvo)?;
        for entrada in fs::read_dir(origem)? {
            let entrada = entrada?;
            copiar_arvore(&entrada.path(), &alvo.join(entrada.file_name()), restante)?;
        }
        return Ok(());
    }
    if !tipo.is_file() {
        // Atalho simbólico ou dispositivo: o serviço também não os envia.
        return Ok(());
    }
    *restante = restante.checked_sub(tipo.len()).ok_or_else(|| {
        io::Error::other(format!(
            "passa de {} GB, o máximo que se copia de uma pasta de rede",
            TETO / (1024 * 1024 * 1024)
        ))
    })?;
    fs::copy(origem, alvo)?;
    Ok(())
}

/// Um nome de pasta novo a cada cópia, para duas cópias seguidas não se misturarem.
fn carimbo() -> String {
    let agora = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default();
    format!("{}-{}", agora.as_millis(), std::process::id())
}

/// Apaga as cópias com mais que [`VALIDADE`]; as recentes podem estar sendo enviadas.
fn limpar_antigas(raiz: &Path) {
    let Ok(entradas) = fs::read_dir(raiz) else {
        return;
    };
    for entrada in entradas.flatten() {
        let velha = entrada
            .metadata()
            .and_then(|m| m.modified())
            .ok()
            .and_then(|quando| quando.elapsed().ok())
            .is_some_and(|idade| idade > VALIDADE);
        if velha {
            let _ = fs::remove_dir_all(entrada.path());
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    fn pasta(nome: &str) -> PathBuf {
        let pasta = std::env::temp_dir().join(format!("ir-rede-{nome}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&pasta);
        fs::create_dir_all(&pasta).unwrap();
        pasta
    }

    #[test]
    fn caminho_local_segue_como_esta() {
        let local = pasta("local").join("a.txt");
        fs::write(&local, b"x").unwrap();
        assert!(!e_de_rede(&local));
        let temporaria = pasta("local-tmp");
        assert_eq!(
            trazer_para_perto(std::slice::from_ref(&local), &temporaria),
            vec![local]
        );
        assert!(!temporaria.join(PASTA).exists(), "nada copiado");
    }

    #[cfg(windows)]
    #[test]
    fn caminho_unc_e_de_rede() {
        assert!(e_de_rede(Path::new(
            r"\\10.0.0.200\Downloads\captura\a.png"
        )));
        assert!(e_de_rede(Path::new(r"\\?\UNC\servidor\pasta\a.png")));
        assert!(!e_de_rede(Path::new(r"C:\Users\a.png")));
    }

    #[cfg(not(windows))]
    #[test]
    fn montagem_do_gvfs_e_de_rede() {
        assert!(e_de_rede(Path::new(
            "/run/user/1000/gvfs/smb-share:server=nas,share=fotos/a.png"
        )));
        assert!(!e_de_rede(Path::new("/home/maxuel/gvfs/a.png")));
    }

    /// Contra um compartilhamento de verdade: `IR_TESTE_REDE=<arquivo numa pasta de rede>`.
    #[test]
    #[ignore = "precisa de uma pasta de rede"]
    fn um_arquivo_de_uma_pasta_de_rede_de_verdade_vem_para_perto() {
        let origem = PathBuf::from(std::env::var("IR_TESTE_REDE").unwrap());
        assert!(e_de_rede(&origem));
        let temporaria = pasta("rede-de-verdade");
        let [perto] = trazer_para_perto(std::slice::from_ref(&origem), &temporaria)
            .try_into()
            .unwrap();
        assert!(perto.starts_with(&temporaria), "{}", perto.display());
        assert!(!e_de_rede(&perto));
        assert_eq!(fs::read(&perto).unwrap(), fs::read(&origem).unwrap());
        let _ = fs::remove_dir_all(temporaria);
    }

    #[test]
    fn uma_pasta_e_copiada_inteira_e_o_teto_vale() {
        let origem = pasta("arvore");
        fs::create_dir_all(origem.join("sub")).unwrap();
        fs::write(origem.join("um.txt"), b"12345").unwrap();
        fs::write(origem.join("sub").join("dois.txt"), b"678").unwrap();
        let destino = pasta("arvore-destino");

        let mut folga = 100;
        let copia = copiar_para(&origem, &destino, &mut folga).unwrap();
        assert_eq!(
            fs::read(copia.join("sub").join("dois.txt")).unwrap(),
            b"678"
        );
        assert_eq!(folga, 92);

        let mut pouco = 6;
        let outro = pasta("arvore-curta");
        assert!(copiar_para(&origem, &outro, &mut pouco).is_err());
        assert!(
            fs::read_dir(&outro).unwrap().next().is_none(),
            "a cópia pela metade não fica"
        );
    }
}
