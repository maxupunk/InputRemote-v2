//! As operações do sistema de arquivos: cada uma vira a mesma operação no cache.
//!
//! O que é deste módulo, e não do cache, são três coisas:
//!
//! - abrir um arquivo sem conteúdo espera ele chegar ([`super::Comum::buscar`]);
//! - a pasta de controle da sincronia (`.inputremote`) não aparece;
//! - um indexador ou gerador de miniaturas não traz conteúdo: abrir, para ele, é recusado. Sem isso,
//!   olhar a pasta no Nautilus baixaria cada imagem para desenhar o ícone.

use std::ffi::OsStr;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::os::unix::fs::{FileExt, PermissionsExt};
use std::time::SystemTime;

use fuser::{
    Errno, FileAttr, FileHandle, FileType, Filesystem, FopenFlags, Generation, INodeNo, LockOwner,
    OpenFlags, RenameFlags, ReplyAttr, ReplyCreate, ReplyData, ReplyDirectory, ReplyEmpty,
    ReplyEntry, ReplyOpen, ReplyStatfs, ReplyWrite, Request, TimeOrNow, WriteFlags,
};

use super::inodes::juntar;
use super::sem_conteudo;
use super::sistema::{Sistema, TTL, tempo};

impl Filesystem for Sistema {
    fn lookup(&self, req: &Request, parent: INodeNo, name: &OsStr, reply: ReplyEntry) {
        match self
            .entrada(req.pid(), parent, name)
            .and_then(|c| self.atributos(&c))
        {
            Ok(attr) => reply.entry(&TTL, &attr, Generation(0)),
            Err(erro) => reply.error(erro),
        }
    }

    fn getattr(&self, _req: &Request, ino: INodeNo, _fh: Option<FileHandle>, reply: ReplyAttr) {
        match self
            .caminho(ino)
            .ok_or(Errno::ENOENT)
            .and_then(|c| self.atributos(&c))
        {
            Ok(attr) => reply.attr(&TTL, &attr),
            Err(erro) => reply.error(erro),
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn setattr(
        &self,
        req: &Request,
        ino: INodeNo,
        mode: Option<u32>,
        _uid: Option<u32>,
        _gid: Option<u32>,
        size: Option<u64>,
        _atime: Option<TimeOrNow>,
        mtime: Option<TimeOrNow>,
        _ctime: Option<SystemTime>,
        _fh: Option<FileHandle>,
        _crtime: Option<SystemTime>,
        _chgtime: Option<SystemTime>,
        _bkuptime: Option<SystemTime>,
        _flags: Option<fuser::BsdFileFlags>,
        reply: ReplyAttr,
    ) {
        let Some(relativo) = self.caminho(ino) else {
            return reply.error(Errno::ENOENT);
        };
        // Encurtar para um tamanho que não é zero precisa do conteúdo: espera noutra thread.
        let espera = size.is_some_and(|t| t != 0) && sem_conteudo(&self.no_cache(&relativo));
        let sistema = self.clone();
        let pid = req.pid();
        let fazer = move || match sistema.mudar_atributos(pid, &relativo, (mode, size, mtime)) {
            Ok(attr) => reply.attr(&TTL, &attr),
            Err(erro) => reply.error(erro),
        };
        if espera {
            std::thread::spawn(fazer);
        } else {
            fazer();
        }
    }

    fn mkdir(
        &self,
        req: &Request,
        parent: INodeNo,
        name: &OsStr,
        mode: u32,
        _umask: u32,
        reply: ReplyEntry,
    ) {
        let feito = self.entrada(req.pid(), parent, name).and_then(|relativo| {
            std::fs::create_dir(self.no_cache(&relativo))?;
            let _ = std::fs::set_permissions(
                self.no_cache(&relativo),
                std::fs::Permissions::from_mode(mode),
            );
            self.atributos(&relativo)
        });
        match feito {
            Ok(attr) => reply.entry(&TTL, &attr, Generation(0)),
            Err(erro) => reply.error(erro),
        }
    }

    fn unlink(&self, req: &Request, parent: INodeNo, name: &OsStr, reply: ReplyEmpty) {
        self.remover((req.pid(), parent), name, reply, |c| {
            std::fs::remove_file(c)
        });
    }

    fn rmdir(&self, req: &Request, parent: INodeNo, name: &OsStr, reply: ReplyEmpty) {
        self.remover((req.pid(), parent), name, reply, |c| std::fs::remove_dir(c));
    }

    fn rename(
        &self,
        req: &Request,
        parent: INodeNo,
        name: &OsStr,
        newparent: INodeNo,
        newname: &OsStr,
        _flags: RenameFlags,
        reply: ReplyEmpty,
    ) {
        let feito = (|| -> Result<(), Errno> {
            let de = self.entrada(req.pid(), parent, name)?;
            let para = self.entrada(req.pid(), newparent, newname)?;
            std::fs::rename(self.no_cache(&de), self.no_cache(&para))?;
            if let Ok(mut inodes) = self.inodes.lock() {
                inodes.renomear(&de, &para);
            }
            Ok(())
        })();
        match feito {
            Ok(()) => reply.ok(),
            Err(erro) => reply.error(erro),
        }
    }

    fn open(&self, req: &Request, ino: INodeNo, flags: OpenFlags, reply: ReplyOpen) {
        match self
            .caminho(ino)
            .ok_or(Errno::ENOENT)
            .and_then(|c| self.pode_abrir(req, &c, flags))
        {
            Ok(()) => reply.opened(FileHandle(0), FopenFlags::empty()),
            Err(erro) => reply.error(erro),
        }
    }

    fn read(
        &self,
        req: &Request,
        ino: INodeNo,
        _fh: FileHandle,
        offset: u64,
        size: u32,
        _flags: OpenFlags,
        _lock_owner: Option<LockOwner>,
        reply: ReplyData,
    ) {
        let Some(relativo) = self.caminho(ino) else {
            return reply.error(Errno::ENOENT);
        };
        // Um arquivo que não veio pode esperar a rede: noutra thread, com a do FUSE livre.
        let espera = sem_conteudo(&self.no_cache(&relativo));
        let sistema = self.clone();
        let pid = req.pid();
        let fazer = move || match sistema.ler(pid, &relativo, (offset, size)) {
            Ok(dados) => reply.data(&dados),
            Err(erro) => reply.error(erro),
        };
        if espera {
            std::thread::spawn(fazer);
        } else {
            fazer();
        }
    }

    fn write(
        &self,
        req: &Request,
        ino: INodeNo,
        _fh: FileHandle,
        offset: u64,
        data: &[u8],
        _write_flags: WriteFlags,
        _flags: OpenFlags,
        _lock_owner: Option<LockOwner>,
        reply: ReplyWrite,
    ) {
        let Some(relativo) = self.caminho(ino) else {
            return reply.error(Errno::ENOENT);
        };
        // Escrever no meio de um arquivo que não veio pede o resto dele primeiro, noutra thread.
        let espera = sem_conteudo(&self.no_cache(&relativo));
        let sistema = self.clone();
        let pid = req.pid();
        let dados = data.to_vec();
        let fazer = move || {
            let escrito = sistema.garantir_conteudo(pid, &relativo).and_then(|()| {
                let arquivo = OpenOptions::new()
                    .write(true)
                    .open(sistema.no_cache(&relativo))?;
                arquivo.write_all_at(&dados, offset)?;
                Ok(())
            });
            match escrito {
                Ok(()) => reply.written(u32::try_from(dados.len()).unwrap_or(u32::MAX)),
                Err(erro) => reply.error(erro),
            }
        };
        if espera {
            std::thread::spawn(fazer);
        } else {
            fazer();
        }
    }

    fn flush(
        &self,
        _req: &Request,
        _ino: INodeNo,
        _fh: FileHandle,
        _lock_owner: LockOwner,
        reply: ReplyEmpty,
    ) {
        reply.ok();
    }

    fn fsync(
        &self,
        _req: &Request,
        _ino: INodeNo,
        _fh: FileHandle,
        _datasync: bool,
        reply: ReplyEmpty,
    ) {
        reply.ok();
    }

    fn readdir(
        &self,
        _req: &Request,
        ino: INodeNo,
        _fh: FileHandle,
        offset: u64,
        mut reply: ReplyDirectory,
    ) {
        let Some(pasta) = self.caminho(ino) else {
            return reply.error(Errno::ENOENT);
        };
        let Ok(lidas) = std::fs::read_dir(self.no_cache(&pasta)) else {
            return reply.error(Errno::ENOENT);
        };
        let mut entradas: Vec<(String, FileType)> = lidas
            .filter_map(Result::ok)
            .filter_map(|e| {
                let nome = e.file_name().to_str()?.to_owned();
                let pasta_de_controle = pasta.is_empty() && nome == ".inputremote";
                let tipo = if e.file_type().ok()?.is_dir() {
                    FileType::Directory
                } else {
                    FileType::RegularFile
                };
                (!pasta_de_controle).then_some((nome, tipo))
            })
            .collect();
        entradas.sort_by(|a, b| a.0.cmp(&b.0));
        let mut todas = vec![
            (".".to_owned(), FileType::Directory),
            ("..".to_owned(), FileType::Directory),
        ];
        todas.extend(entradas);
        for (posicao, (nome, tipo)) in todas
            .into_iter()
            .enumerate()
            .skip(usize::try_from(offset).unwrap_or(0))
        {
            let numero = match nome.as_str() {
                "." | ".." => ino.0,
                _ => self
                    .inodes
                    .lock()
                    .map_or(0, |mut i| i.numero(&juntar(&pasta, &nome))),
            };
            if reply.add(INodeNo(numero), (posicao + 1) as u64, tipo, &nome) {
                break;
            }
        }
        reply.ok();
    }

    fn statfs(&self, _req: &Request, _ino: INodeNo, reply: ReplyStatfs) {
        let espaco = super::espaco_livre(&self.comum.conteudo);
        reply.statfs(espaco.0, espaco.1, espaco.1, 0, 0, 4096, 255, 4096);
    }

    fn access(&self, _req: &Request, _ino: INodeNo, _mask: fuser::AccessFlags, reply: ReplyEmpty) {
        reply.ok();
    }

    fn create(
        &self,
        req: &Request,
        parent: INodeNo,
        name: &OsStr,
        mode: u32,
        _umask: u32,
        _flags: i32,
        reply: ReplyCreate,
    ) {
        let feito = self.entrada(req.pid(), parent, name).and_then(|relativo| {
            let caminho = self.no_cache(&relativo);
            OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&caminho)?
                .flush()?;
            let _ = std::fs::set_permissions(&caminho, std::fs::Permissions::from_mode(mode));
            self.atributos(&relativo)
        });
        match feito {
            Ok(attr) => reply.created(
                &TTL,
                &attr,
                Generation(0),
                FileHandle(0),
                FopenFlags::empty(),
            ),
            Err(erro) => reply.error(erro),
        }
    }
}

impl Sistema {
    /// O que o `setattr` muda: tamanho, permissões, horário.
    fn mudar_atributos(
        &self,
        pid: u32,
        relativo: &str,
        (mode, size, mtime): (Option<u32>, Option<u64>, Option<TimeOrNow>),
    ) -> Result<FileAttr, Errno> {
        let caminho = self.no_cache(relativo);
        if let Some(tamanho) = size {
            // Esvaziar não precisa do conteúdo antigo — é o `>` do terminal e o "salvar por
            // cima" de quase todo programa, e funciona offline. Encurtar para outro tamanho
            // precisa do que fica.
            if tamanho == 0 && sem_conteudo(&caminho) {
                super::marcar_sem_conteudo(&caminho, false)?;
            } else {
                self.garantir_conteudo(pid, relativo)?;
            }
            OpenOptions::new()
                .write(true)
                .open(&caminho)?
                .set_len(tamanho)?;
        }
        if let Some(modo) = mode {
            std::fs::set_permissions(&caminho, std::fs::Permissions::from_mode(modo))?;
        }
        if let Some(quando) = mtime {
            File::options()
                .write(true)
                .open(&caminho)?
                .set_modified(tempo(quando))?;
        }
        self.atributos(relativo)
    }
}
