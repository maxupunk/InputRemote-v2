//! A pasta recebida sob demanda no Linux: um sistema de arquivos em espaço de usuário (FUSE).
//!
//! A réplica guarda o conteúdo num cache escondido, na pasta de estado do usuário, e a pasta que a
//! pessoa vê (`~/InputRemote/<nome>`) é este sistema de arquivos mostrando o cache. Um arquivo que
//! ainda não veio é, no cache, um arquivo esparso com o tamanho e a data certos e uma marca no
//! atributo estendido `user.inputremote.sem_conteudo`. Abrir um deles pede o conteúdo ao ajudante
//! ([`crate::Pedido::Buscar`]) e espera ele chegar ([`Montagem::pronto`]).
//!
//! A marca mora no próprio arquivo, e não numa lista ao lado: um `rename` a leva junto, e o download
//! que substitui o arquivo a apaga sem ninguém precisar lembrar.

#![allow(unsafe_code)]

mod fs;
mod inodes;
mod sistema;
mod trechos;

use std::collections::HashMap;
use std::ffi::CString;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::time::Duration;

use crate::Pedido;

pub use trechos::{entregar, falhar};

/// O atributo estendido que marca um arquivo sem conteúdo.
const MARCA: &str = "user.inputremote.sem_conteudo";

/// Quanto quem abriu um arquivo espera o conteúdo chegar antes de ouvir "rede inalcançável".
const PRAZO: Duration = Duration::from_secs(300);

/// Se esta máquina monta sistemas de arquivos de usuário: o dispositivo e o `fusermount3`.
#[must_use]
pub fn suportado(_raiz: &Path) -> bool {
    Path::new("/dev/fuse").exists()
        && std::env::var_os("PATH").is_some_and(|caminhos| {
            std::env::split_paths(&caminhos).any(|dir| dir.join("fusermount3").exists())
        })
}

/// Marca, ou desmarca, um arquivo do cache como sem conteúdo.
///
/// # Errors
///
/// Quando o sistema de arquivos do cache não aceita atributo estendido de usuário.
pub fn marcar_sem_conteudo(caminho: &Path, sem: bool) -> std::io::Result<()> {
    let alvo = CString::new(caminho.as_os_str().as_bytes())?;
    let nome = CString::new(MARCA)?;
    let feito = if sem {
        let valor = b"1";
        // SAFETY: os dois textos terminam em zero e vivem até o fim; o valor é local.
        unsafe {
            libc::setxattr(
                alvo.as_ptr(),
                nome.as_ptr(),
                valor.as_ptr().cast(),
                valor.len(),
                0,
            )
        }
    } else {
        // SAFETY: como acima.
        let r = unsafe { libc::removexattr(alvo.as_ptr(), nome.as_ptr()) };
        if r != 0 && std::io::Error::last_os_error().raw_os_error() == Some(libc::ENODATA) {
            0
        } else {
            r
        }
    };
    if feito == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

/// Se o arquivo do cache está sem conteúdo.
#[must_use]
pub fn sem_conteudo(caminho: &Path) -> bool {
    let (Ok(alvo), Ok(nome)) = (
        CString::new(caminho.as_os_str().as_bytes()),
        CString::new(MARCA),
    ) else {
        return false;
    };
    // SAFETY: só pergunta o tamanho do valor; sem buffer.
    unsafe { libc::getxattr(alvo.as_ptr(), nome.as_ptr(), std::ptr::null_mut(), 0) >= 0 }
}

/// Blocos de 4 KiB no disco do cache: o total e os livres, para o `df` da pasta montada.
pub(crate) fn espaco_livre(caminho: &Path) -> (u64, u64) {
    let Ok(alvo) = CString::new(caminho.as_os_str().as_bytes()) else {
        return (0, 0);
    };
    // SAFETY: a estrutura é local e o sistema só escreve nela; o caminho termina em zero.
    let mut dados: libc::statvfs = unsafe { std::mem::zeroed() };
    // SAFETY: como acima.
    if unsafe { libc::statvfs(alvo.as_ptr(), &raw mut dados) } != 0 {
        return (0, 0);
    }
    let fator = (dados.f_frsize / 4096).max(1);
    (
        dados.f_blocks.saturating_mul(fator),
        dados.f_bavail.saturating_mul(fator),
    )
}

/// Um conteúdo pedido, e a resposta que ainda não chegou.
#[derive(Default)]
struct Espera {
    resposta: Mutex<Option<bool>>,
    chegou: Condvar,
}

/// O que o sistema de arquivos e o ajudante dividem.
pub(crate) struct Comum {
    /// O cache, onde o conteúdo de verdade está.
    conteudo: PathBuf,
    /// O ponto de montagem, que vai em cada pedido para o ajudante achar a pasta.
    ponto: PathBuf,
    repassar: Box<dyn Fn(Pedido) + Send + Sync>,
    esperas: Mutex<HashMap<String, Arc<Espera>>>,
    /// Por onde se diz ao núcleo que um arquivo mudou por baixo dele. Existe depois de montar.
    nucleo: OnceLock<fuser::Notifier>,
    /// Se o outro computador está ao alcance. Sem ele, abrir um arquivo que não veio falha na hora,
    /// com "rede inalcançável" — na leitura, o núcleo trocaria o motivo por "erro de E/S".
    alcance: AtomicBool,
}

impl Comum {
    /// Pede só um trecho de `caminho` e espera; o arquivo continua sem conteúdo.
    fn trecho(&self, caminho: &str, offset: u64, tamanho: u64) -> Option<Vec<u8>> {
        let pedir = |transferencia| {
            (self.repassar)(Pedido::Buscar(crate::Busca {
                raiz: self.ponto.clone(),
                caminho: caminho.to_owned(),
                conexao: 0,
                transferencia,
                offset,
                tamanho,
            }));
        };
        trechos::ler(pedir, offset, tamanho)
    }

    /// Pede o conteúdo de `caminho` e espera. `true` se chegou.
    fn buscar(&self, caminho: &str) -> bool {
        let (espera, primeiro) = {
            let Ok(mut esperas) = self.esperas.lock() else {
                return false;
            };
            if let Some(espera) = esperas.get(caminho) {
                (Arc::clone(espera), false)
            } else {
                let espera = Arc::new(Espera::default());
                esperas.insert(caminho.to_owned(), Arc::clone(&espera));
                (espera, true)
            }
        };
        if primeiro {
            (self.repassar)(Pedido::Buscar(crate::Busca {
                raiz: self.ponto.clone(),
                caminho: caminho.to_owned(),
                conexao: 0,
                transferencia: 0,
                offset: 0,
                tamanho: u64::MAX,
            }));
        }
        let Ok(resposta) = espera.resposta.lock() else {
            return false;
        };
        let Ok((resposta, _)) = espera
            .chegou
            .wait_timeout_while(resposta, PRAZO, |r| r.is_none())
        else {
            return false;
        };
        resposta.unwrap_or(false)
    }

    /// O conteúdo chegou: o núcleo esquece os atributos que guardou do marcador. Se a origem mudou no
    /// meio e o tamanho é outro, sem isso ele leria só até o tamanho antigo.
    fn atributos_mudaram(&self, numero: u64) {
        if let Some(nucleo) = self.nucleo.get() {
            // Deslocamento negativo: só os atributos, sem mexer nas páginas.
            let _ = nucleo.inval_inode(fuser::INodeNo(numero), -1, 0);
        }
    }

    fn pronto(&self, caminho: &str, chegou: bool) {
        let espera = self.esperas.lock().ok().and_then(|mut e| e.remove(caminho));
        if let Some(espera) = espera {
            if let Ok(mut resposta) = espera.resposta.lock() {
                *resposta = Some(chegou);
            }
            espera.chegou.notify_all();
        }
    }
}

/// A pasta montada. Desmonta ao sair de escopo.
pub struct Montagem {
    comum: Arc<Comum>,
    _sessao: fuser::BackgroundSession,
}

impl std::fmt::Debug for Montagem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Montagem")
            .field("ponto", &self.comum.ponto)
            .finish_non_exhaustive()
    }
}

impl Montagem {
    /// O outro computador ficou, ou deixou de ficar, ao alcance.
    pub fn alcance(&self, ao_alcance: bool) {
        self.comum.alcance.store(ao_alcance, Ordering::Relaxed);
    }

    /// O conteúdo pedido chegou ao cache — ou não vem.
    pub fn pronto(&self, caminho: &str, chegou: bool) {
        self.comum.pronto(caminho, chegou);
    }
}

/// Monta `conteudo` em `ponto`. Abrir um arquivo sem conteúdo chama `repassar`.
///
/// # Errors
///
/// Quando o ponto não existe, já está montado, ou o sistema não monta sistema de arquivos de usuário.
pub fn montar(
    ponto: &Path,
    conteudo: &Path,
    repassar: impl Fn(Pedido) + Send + Sync + 'static,
) -> std::io::Result<Montagem> {
    // Um ponto que ficou montado de uma execução que morreu: desmontar antes de montar de novo, e
    // antes de olhar o ponto — uma montagem morta responde "não conectado" a qualquer pergunta.
    // Desmontagem preguiçosa (`-z`): com o Nautilus aberto na pasta, a comum diz "ocupado", e a
    // pasta ficava morta até reiniciar o computador.
    let _ = std::process::Command::new("fusermount3")
        .args(["-u", "-z"])
        .arg(ponto)
        .output();
    std::fs::create_dir_all(ponto)?;
    std::fs::create_dir_all(conteudo)?;
    let comum = Arc::new(Comum {
        conteudo: conteudo.to_path_buf(),
        ponto: ponto.to_path_buf(),
        repassar: Box::new(repassar),
        esperas: Mutex::default(),
        nucleo: OnceLock::new(),
        alcance: AtomicBool::new(true),
    });
    let sistema = sistema::Sistema::novo(Arc::clone(&comum));
    let mut config = fuser::Config::default();
    config.mount_options = vec![
        fuser::MountOption::FSName("inputremote".to_owned()),
        // O tipo na tabela de montagens é o que faz o GNOME tratar a pasta como de rede: a GLib só
        // chama de remota `nfs`, `cifs`, `smb` e, entre as FUSE, `fuse.sshfs`
        // (`is_remote_fs_type`, gio/glocalfile.c). Remota, o Nautilus não abre cada arquivo para
        // desenhar a miniatura — com a preferência padrão, "só arquivos locais" —, e o `updatedb`
        // não a indexa. Com `fuse.inputremote`, abrir uma pasta de fotos no Nautilus baixava todas
        // (log 59). A origem continua `inputremote`, que é o que o `findmnt` mostra como fonte.
        fuser::MountOption::Subtype("sshfs".to_owned()),
        fuser::MountOption::DefaultPermissions,
        fuser::MountOption::NoDev,
        fuser::MountOption::NoSuid,
    ];
    // Várias threads: quem espera o conteúdo de um arquivo não trava a pasta para os outros.
    config.n_threads = Some(4);
    let sessao = fuser::Session::new(sistema, ponto, &config)?;
    let _ = comum.nucleo.set(sessao.notifier());
    let sessao = sessao.spawn()?;
    Ok(Montagem {
        comum,
        _sessao: sessao,
    })
}

#[cfg(test)]
mod testes;
