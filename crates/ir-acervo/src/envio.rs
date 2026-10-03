//! Os dois lados de um arquivo que a réplica manda à origem.
//!
//! **Quem manda** ([`Envio`]) tira primeiro uma cópia do arquivo para a montagem, calculando o resumo
//! no caminho: o usuário pode continuar editando o original, e o que vai é um retrato coerente — o
//! resumo confere com os bytes, sempre. Os bytes só saem com crédito da origem.
//!
//! **Quem recebe** ([`Recebimento`]) monta ao lado e libera crédito em pedaços de 4 MiB, à medida que
//! grava. É o que impede um arquivo de vários gigabytes de ocupar o canal 5 inteiro: a cópia do
//! clipboard e os trechos pedidos passam entre um pedaço e outro.

use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use ir_proto::limits::MAX_FILE_BLOCK;
use ir_proto::message::{FolderId, FolderMessage, OpId};

use crate::Saida;

/// Quanto crédito a origem dá de cada vez.
const PASSO_DE_CREDITO: u64 = 4 * 1024 * 1024;

/// Um arquivo indo para a origem.
#[derive(Debug)]
pub struct Envio {
    /// A operação.
    pub op: OpId,
    /// O caminho, para registrar um conflito.
    pub caminho: String,
    /// O resumo do que vai.
    pub resumo: [u8; 32],
    montado: PathBuf,
    tamanho: u64,
    enviado: u64,
    credito: u64,
    fim_mandado: bool,
}

impl Envio {
    /// Tira o retrato do arquivo e anuncia o envio à origem.
    ///
    /// # Errors
    ///
    /// Erro de disco: o arquivo sumiu, ou está aberto com exclusividade por outro programa.
    pub fn preparar(
        raiz: &Path,
        (op, caminho, base): (OpId, &str, u64),
        pasta: FolderId,
        saida: &mut dyn Saida,
    ) -> std::io::Result<Self> {
        let original = crate::disco::absoluto(raiz, caminho);
        let horario = crate::varredura::horario(&std::fs::metadata(&original)?);
        let montado = crate::disco::arquivo_de_montagem(raiz, &format!("envio-{}", op.0))?;
        let (tamanho, resumo) = match retratar(&original, &montado) {
            Ok(feito) => feito,
            Err(erro) => {
                let _ = std::fs::remove_file(&montado);
                return Err(erro);
            }
        };
        saida.enviar(FolderMessage::Upload {
            folder: pasta,
            op,
            path: caminho.to_owned(),
            base,
            size: tamanho,
            hash: resumo,
            modified_ns: horario,
        });
        Ok(Self {
            op,
            caminho: caminho.to_owned(),
            resumo,
            montado,
            tamanho,
            enviado: 0,
            credito: 0,
            fim_mandado: false,
        })
    }

    /// A origem liberou mais bytes: manda o que couber, e o fim quando acabar.
    ///
    /// # Errors
    ///
    /// Erro de disco ao ler a montagem.
    pub fn creditar(
        &mut self,
        bytes: u32,
        pasta: FolderId,
        saida: &mut dyn Saida,
    ) -> std::io::Result<()> {
        self.credito += u64::from(bytes);
        let mut arquivo = std::fs::File::open(&self.montado)?;
        arquivo.seek(SeekFrom::Start(self.enviado))?;
        let mut bloco = vec![0u8; MAX_FILE_BLOCK];
        while self.enviado < self.tamanho && self.credito > 0 {
            let cabe = usize::try_from(self.credito.min(self.tamanho - self.enviado))
                .unwrap_or(MAX_FILE_BLOCK)
                .min(MAX_FILE_BLOCK);
            let lidos = arquivo.read(bloco.get_mut(..cabe).unwrap_or_default())?;
            if lidos == 0 {
                return Err(std::io::ErrorKind::UnexpectedEof.into());
            }
            saida.enviar(FolderMessage::UploadBlock {
                folder: pasta,
                op: self.op,
                offset: self.enviado,
                data: bloco.get(..lidos).unwrap_or_default().to_vec(),
            });
            self.enviado += lidos as u64;
            self.credito -= lidos as u64;
        }
        if self.enviado >= self.tamanho && !self.fim_mandado {
            self.fim_mandado = true;
            saida.enviar(FolderMessage::UploadEnd {
                folder: pasta,
                op: self.op,
            });
        }
        Ok(())
    }

    /// O envio acabou, de um jeito ou de outro: a montagem sai.
    pub fn descartar(self) {
        let _ = std::fs::remove_file(&self.montado);
    }
}

/// Copia `original` para `montado` calculando o resumo. Devolve o tamanho e o resumo.
fn retratar(original: &Path, montado: &Path) -> std::io::Result<(u64, [u8; 32])> {
    let mut de = std::fs::File::open(original)?;
    let mut para = std::fs::File::create(montado)?;
    let mut resumo = blake3::Hasher::new();
    let mut bloco = vec![0u8; 256 * 1024];
    let mut tamanho = 0u64;
    loop {
        let lidos = de.read(&mut bloco)?;
        if lidos == 0 {
            break;
        }
        let pedaco = bloco.get(..lidos).unwrap_or_default();
        para.write_all(pedaco)?;
        resumo.update(pedaco);
        tamanho += lidos as u64;
    }
    para.sync_all()?;
    Ok((tamanho, *resumo.finalize().as_bytes()))
}

/// Um arquivo chegando da réplica, na origem.
#[derive(Debug)]
pub struct Recebimento {
    /// O que a réplica anunciou.
    pub anuncio: ir_pasta::EnvioRecebido,
    /// Onde está sendo montado.
    pub montado: PathBuf,
    arquivo: Option<std::fs::File>,
    recebido: u64,
    liberado: u64,
}

impl Recebimento {
    /// Abre a montagem e dá o primeiro crédito.
    ///
    /// # Errors
    ///
    /// Erro de disco ao criar a montagem.
    pub fn abrir(
        raiz: &Path,
        (op, anuncio): (OpId, ir_pasta::EnvioRecebido),
        pasta: FolderId,
        saida: &mut dyn Saida,
    ) -> std::io::Result<Self> {
        let montado = crate::disco::arquivo_de_montagem(raiz, &format!("recebe-{}", op.0))?;
        let arquivo = std::fs::File::create(&montado)?;
        let mut recebimento = Self {
            anuncio,
            montado,
            arquivo: Some(arquivo),
            recebido: 0,
            liberado: 0,
        };
        recebimento.liberar(op, pasta, saida);
        Ok(recebimento)
    }

    /// Um bloco chegou.
    ///
    /// # Errors
    ///
    /// Erro de disco, ou um bloco fora de ordem ou além do anunciado.
    pub fn bloco(
        &mut self,
        (op, offset, dados): (OpId, u64, &[u8]),
        pasta: FolderId,
        saida: &mut dyn Saida,
    ) -> std::io::Result<()> {
        let alem = self.recebido + dados.len() as u64 > self.anuncio.tamanho;
        if offset != self.recebido || alem {
            return Err(std::io::ErrorKind::InvalidData.into());
        }
        let arquivo = self
            .arquivo
            .as_mut()
            .ok_or(std::io::ErrorKind::BrokenPipe)?;
        arquivo.write_all(dados)?;
        self.recebido += dados.len() as u64;
        if self.recebido + PASSO_DE_CREDITO / 2 >= self.liberado {
            self.liberar(op, pasta, saida);
        }
        Ok(())
    }

    /// Se chegou tudo, e o resumo confere com o anunciado.
    pub fn conferir(&mut self) -> bool {
        if let Some(arquivo) = self.arquivo.take()
            && arquivo.sync_all().is_err()
        {
            return false;
        }
        self.recebido == self.anuncio.tamanho
            && crate::varredura::resumir(&self.montado).is_ok_and(|r| r == self.anuncio.resumo)
    }

    fn liberar(&mut self, op: OpId, pasta: FolderId, saida: &mut dyn Saida) {
        let falta = self.anuncio.tamanho.saturating_sub(self.liberado);
        if falta == 0 {
            return;
        }
        let passo = falta.min(PASSO_DE_CREDITO);
        self.liberado += passo;
        saida.enviar(FolderMessage::Credit {
            folder: pasta,
            op,
            bytes: u32::try_from(passo).unwrap_or(u32::MAX),
        });
    }
}
