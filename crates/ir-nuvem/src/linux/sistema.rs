//! O sistema de arquivos e o que ele sabe do cache: o caminho de cada inode, os atributos, e quem
//! precisa do conteúdo antes de abrir. As operações em si estão em [`super::fs`].

use std::ffi::OsStr;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use fuser::{Errno, FileAttr, FileType, INodeNo, ReplyEmpty, Request, TimeOrNow};

use super::inodes::{Inodes, juntar};
use super::{Comum, sem_conteudo, trechos};

/// Por quanto tempo o núcleo pode guardar um atributo sem perguntar de novo.
pub(super) const TTL: Duration = Duration::from_secs(1);

/// Os indexadores, que leem arquivos sem a pessoa ter pedido: abrir, para eles, não traz conteúdo.
/// Pelo nome em `/proc/<pid>/comm`, que o núcleo corta em 15 caracteres.
const INDEXADORES: [&str; 4] = [
    "tracker-extract",
    "tracker-miner-f",
    "localsearch-ext",
    "localsearch-3",
];

pub(super) struct Sistema {
    pub(super) comum: Arc<Comum>,
    pub(super) inodes: Mutex<Inodes>,
}

impl Sistema {
    pub(super) fn novo(comum: Arc<Comum>) -> Self {
        Self {
            comum,
            inodes: Mutex::new(Inodes::novos()),
        }
    }

    pub(super) fn caminho(&self, ino: INodeNo) -> Option<String> {
        self.inodes.lock().ok()?.caminho(ino.0)
    }

    pub(super) fn no_cache(&self, relativo: &str) -> PathBuf {
        relativo
            .split('/')
            .filter(|p| !p.is_empty())
            .fold(self.comum.conteudo.clone(), |c, p| c.join(p))
    }

    pub(super) fn atributos(&self, relativo: &str) -> Result<FileAttr, Errno> {
        let dados = std::fs::symlink_metadata(self.no_cache(relativo)).map_err(Errno::from)?;
        let ino = self.inodes.lock().map_err(|_| Errno::EIO)?.numero(relativo);
        let quando = |s: i64, ns: i64| {
            UNIX_EPOCH
                + Duration::new(
                    u64::try_from(s).unwrap_or(0),
                    u32::try_from(ns).unwrap_or(0),
                )
        };
        Ok(FileAttr {
            ino: INodeNo(ino),
            size: dados.len(),
            blocks: dados.blocks(),
            atime: quando(dados.atime(), dados.atime_nsec()),
            mtime: quando(dados.mtime(), dados.mtime_nsec()),
            ctime: quando(dados.ctime(), dados.ctime_nsec()),
            crtime: quando(dados.mtime(), dados.mtime_nsec()),
            kind: if dados.is_dir() {
                FileType::Directory
            } else {
                FileType::RegularFile
            },
            perm: u16::try_from(dados.permissions().mode() & 0o7777).unwrap_or(0o644),
            nlink: u32::try_from(dados.nlink()).unwrap_or(1),
            uid: dados.uid(),
            gid: dados.gid(),
            rdev: 0,
            blksize: 4096,
            flags: 0,
        })
    }

    /// Abrir um arquivo que não veio não traz nada: só o gerador de miniaturas e o indexador ouvem
    /// "acesso negado". O conteúdo vem na leitura ([`Self::ler`]) ou na escrita.
    pub(super) fn pode_abrir(
        &self,
        req: &Request,
        relativo: &str,
        flags: fuser::OpenFlags,
    ) -> Result<(), Errno> {
        if !sem_conteudo(&self.no_cache(relativo)) {
            return Ok(());
        }
        let (processo, fio) = quem_abriu(req.pid());
        if de_fundo_pelo_nome(&processo) || de_fundo_pelo_nome(&fio) {
            tracing::debug!(
                processo,
                fio,
                "abrir sem conteúdo recusado: miniatura ou índice"
            );
            return Err(Errno::EACCES);
        }
        // Só quem abre para ler: abrir para gravar por cima (`O_TRUNC`, que o núcleo manda antes
        // de esvaziar) funciona offline.
        let so_ler = flags.0 & libc::O_ACCMODE == libc::O_RDONLY && flags.0 & libc::O_TRUNC == 0;
        let ao_alcance = self
            .comum
            .alcance
            .load(std::sync::atomic::Ordering::Relaxed);
        if so_ler && !ao_alcance {
            return Err(Errno::ENETUNREACH);
        }
        Ok(())
    }

    /// Lê do cache. Num arquivo que não veio, ler só o começo traz só o trecho, da origem; ler além
    /// dele, ou até o fim, traz o arquivo inteiro antes.
    pub(super) fn ler(
        &self,
        req: &Request,
        relativo: &str,
        (offset, tamanho): (u64, u32),
    ) -> Result<Vec<u8>, Errno> {
        let caminho = self.no_cache(relativo);
        if sem_conteudo(&caminho) {
            let total = std::fs::metadata(&caminho)?.len();
            let fim = offset.saturating_add(u64::from(tamanho)).min(total);
            if fim <= offset {
                return Ok(Vec::new());
            }
            // Só o começo de um arquivo maior é trecho. Quem chega ao fim leu o arquivo — um
            // arquivo pequeno lido inteiro fica no disco, para abrir de novo sem rede.
            if fim <= trechos::COMECO && fim < total {
                return self
                    .comum
                    .trecho(relativo, offset, fim - offset)
                    .ok_or(Errno::ENETUNREACH);
            }
            self.garantir_conteudo(req, relativo)?;
        }
        let mut arquivo = File::open(&caminho)?;
        arquivo.seek(SeekFrom::Start(offset))?;
        let mut dados = Vec::with_capacity(usize::try_from(tamanho).unwrap_or(0));
        arquivo.take(u64::from(tamanho)).read_to_end(&mut dados)?;
        Ok(dados)
    }

    /// Traz o conteúdo inteiro, se ele ainda não veio. `Err` quando não vem.
    pub(super) fn garantir_conteudo(&self, req: &Request, relativo: &str) -> Result<(), Errno> {
        if !sem_conteudo(&self.no_cache(relativo)) {
            return Ok(());
        }
        let (processo, fio) = quem_abriu(req.pid());
        if de_fundo_pelo_nome(&processo) || de_fundo_pelo_nome(&fio) {
            tracing::debug!(
                processo,
                fio,
                "abrir sem conteúdo recusado: miniatura ou índice"
            );
            return Err(Errno::EACCES);
        }
        // Quem abriu, e nunca o nome do arquivo (docs/04, §7): é o que diz, num log, quem baixou.
        tracing::info!(
            processo,
            fio,
            "um arquivo sem conteúdo foi aberto; buscando"
        );
        if !self.comum.buscar(relativo) {
            return Err(Errno::ENETUNREACH);
        }
        let numero = self.inodes.lock().map_err(|_| Errno::EIO)?.numero(relativo);
        self.comum.atributos_mudaram(numero);
        Ok(())
    }

    pub(super) fn entrada(&self, pai: INodeNo, nome: &OsStr) -> Result<String, Errno> {
        let pai = self.caminho(pai).ok_or(Errno::ENOENT)?;
        let nome = nome.to_str().ok_or(Errno::EINVAL)?;
        if pai.is_empty() && nome == ".inputremote" {
            return Err(Errno::ENOENT);
        }
        Ok(juntar(&pai, nome))
    }
}

/// O nome do processo e o da thread que abriu; vazios se ela já saiu.
///
/// O FUSE dá o número da **thread**, e o `comm` dela é o nome da thread — "pool-23", num programa
/// da `GLib` —, não o do programa. O programa vem do `Tgid` em `/proc/<tid>/status`.
fn quem_abriu(tid: u32) -> (String, String) {
    let nome = |id: &str| {
        std::fs::read_to_string(format!("/proc/{id}/comm"))
            .map(|n| n.trim().to_owned())
            .unwrap_or_default()
    };
    let fio = nome(&tid.to_string());
    let dono = std::fs::read_to_string(format!("/proc/{tid}/status"))
        .ok()
        .and_then(|status| {
            status
                .lines()
                .find_map(|linha| linha.strip_prefix("Tgid:").map(|v| v.trim().to_owned()))
        });
    let processo = dono.map_or_else(|| fio.clone(), |dono| nome(&dono));
    (processo, fio)
}

/// Gerador de miniaturas ou indexador, pelo nome do processo.
///
/// Gerador de miniaturas é qualquer um com "thum" no nome, e não uma lista: cada versão do GNOME
/// troca os seus. No Fedora 44 são `glycin-thumbnailer`, `papers-thumbnailer`,
/// `gst-video-thumbnailer` e `gsf-office-thumbnailer`; antes eram `gdk-pixbuf-thumbnailer`,
/// `evince-thumbnailer` e `totem-video-thumbnailer` — e a primeira lista, só com esses, deixou o
/// Nautilus baixar cada foto de uma pasta só para desenhar o ícone.
fn de_fundo_pelo_nome(nome: &str) -> bool {
    let nome = nome.trim();
    nome.contains("thum") || INDEXADORES.iter().any(|p| nome.starts_with(p))
}

pub(super) fn tempo(valor: TimeOrNow) -> SystemTime {
    match valor {
        TimeOrNow::SpecificTime(quando) => quando,
        TimeOrNow::Now => SystemTime::now(),
    }
}

impl Sistema {
    pub(super) fn remover(
        &self,
        parent: INodeNo,
        name: &OsStr,
        reply: ReplyEmpty,
        tirar: impl FnOnce(&std::path::Path) -> std::io::Result<()>,
    ) {
        let feito = self.entrada(parent, name).and_then(|relativo| {
            tirar(&self.no_cache(&relativo))?;
            if let Ok(mut inodes) = self.inodes.lock() {
                inodes.esquecer(&relativo);
            }
            Ok(())
        });
        match feito {
            Ok(()) => reply.ok(),
            Err(erro) => reply.error(erro),
        }
    }
}

#[cfg(test)]
mod testes {
    use super::de_fundo_pelo_nome;

    #[test]
    fn miniaturas_e_indexadores_nao_baixam_e_o_resto_baixa() {
        // Como aparecem em `/proc/<pid>/comm`: cortados em 15 caracteres, com a quebra de linha.
        for de_fundo in [
            "glycin-thumbnai\n",
            "papers-thumbnai\n",
            "gst-video-thumb\n",
            "gsf-office-thum\n",
            "gnome-thumbnail\n",
            "gdk-pixbuf-thum\n",
            "ffmpegthumbnail\n",
            "localsearch-ext\n",
            "tracker-miner-f\n",
        ] {
            assert!(de_fundo_pelo_nome(de_fundo), "{de_fundo}");
        }
        for pessoa in [
            "nautilus\n",
            "gnome-text-edit\n",
            "cat\n",
            "loupe\n",
            "soffice.bin\n",
        ] {
            assert!(!de_fundo_pelo_nome(pessoa), "{pessoa}");
        }
    }
}
