//! O lado que envia: um plano de transferência virando mensagens do canal 5, uma por vez.
//!
//! # Por que puxar em vez de empurrar
//!
//! Este tipo não conhece socket e não envia nada. Ele responde "a próxima mensagem é esta", e quem
//! chama decide quando pedir a seguinte — o que na prática é quando o socket aceitou a anterior.
//!
//! A contrapressão sai de graça disso. Numa transferência de 5 GB, um produtor que empurrasse
//! blocos numa fila cresceria até a memória acabar sempre que a rede fosse mais lenta que o disco,
//! e ela costuma ser. Aqui o disco nunca vai à frente do socket, porque é o socket que pede.
//!
//! Também é o que mantém a seta de dependência: `ir-files` depende de `ir-proto` e de mais nada
//! ([02, §2](../../../docs/02-arquitetura.md)). Ele não sabe que existe TCP.

use std::path::PathBuf;

use ir_proto::limits;
use ir_proto::message::{BulkMessage, ManifestItem};
use tokio::io::AsyncReadExt;

use crate::error::{FileError, Result};
use crate::manifesto::Plano;

/// O arquivo que está sendo lido agora.
#[derive(Debug)]
struct Aberto {
    indice: usize,
    caminho: PathBuf,
    arquivo: tokio::fs::File,
    lidos: u64,
    declarado: u64,
    resumo: blake3::Hasher,
}

/// Produz as mensagens do corpo de uma transferência.
#[derive(Debug)]
pub struct Envio {
    plano: Plano,
    proximo: usize,
    aberto: Option<Aberto>,
    enviados: u64,
}

impl Envio {
    /// Começa um envio a partir de um plano já montado.
    #[must_use]
    pub const fn novo(plano: Plano) -> Self {
        Self {
            plano,
            proximo: 0,
            aberto: None,
            enviados: 0,
        }
    }

    /// O manifesto, que é a primeira coisa a ir para o par.
    #[must_use]
    pub fn manifesto(&self) -> BulkMessage {
        BulkMessage::Manifest {
            id: self.plano.id,
            items: self.plano.itens.clone(),
            total_bytes: self.plano.total,
        }
    }

    /// O identificador desta transferência, para as mensagens que não saem de [`Self::proxima`] —
    /// o `Cancel`, em especial.
    #[must_use]
    pub const fn id(&self) -> ir_proto::message::TransferId {
        self.plano.id
    }

    /// Quantos bytes de conteúdo já saíram.
    #[must_use]
    pub const fn enviados(&self) -> u64 {
        self.enviados
    }

    /// Quantos bytes de conteúdo o plano tem no total.
    #[must_use]
    pub const fn total(&self) -> u64 {
        self.plano.total
    }

    /// A próxima mensagem do corpo, ou `None` quando não há mais nada a enviar.
    ///
    /// # Errors
    ///
    /// [`FileError::Io`] em falha de leitura; [`FileError::MudouDurante`] se o arquivo encolheu ou
    /// cresceu entre o manifesto e a leitura; [`FileError::Violacao`] se o plano tem mais itens do
    /// que um `u32` endereça — o que seria defeito nosso, e é pego antes de o par ver.
    pub async fn proxima(&mut self) -> Result<Option<BulkMessage>> {
        if self.aberto.is_some() {
            return self.continuar().await;
        }
        self.comecar_o_proximo()
    }

    /// Abre o próximo arquivo do plano, pulando os diretórios.
    ///
    /// Diretório não gera mensagem: quem recebe o cria a partir do manifesto, que ele já tem
    /// inteiro. Mandar `FileStart` de uma pasta seria uma mensagem sem conteúdo e um caso extra na
    /// máquina de estados dos dois lados.
    fn comecar_o_proximo(&mut self) -> Result<Option<BulkMessage>> {
        loop {
            let Some(item) = self.plano.itens.get(self.proximo) else {
                return Ok(None);
            };
            if item.is_dir {
                self.proximo += 1;
                continue;
            }
            let indice = self.proximo;
            let caminho = self
                .plano
                .locais
                .get(indice)
                .ok_or(FileError::Violacao("item sem caminho local"))?
                .clone();
            // Aberto primeiro, conferido depois: o que se confere é o descritor que vai ser lido, e
            // não o caminho — que pode ter sido trocado por um vínculo desde o manifesto
            // ([`crate::permissao`]).
            let aberto =
                std::fs::File::open(&caminho).map_err(|erro| FileError::io(&caminho, erro))?;
            crate::permissao::conferir_aberto(&self.plano.leitor, &caminho, &aberto)?;
            let arquivo = tokio::fs::File::from_std(aberto);
            let declarado = item.size;
            self.aberto = Some(Aberto {
                indice,
                caminho,
                arquivo,
                lidos: 0,
                declarado,
                resumo: blake3::Hasher::new(),
            });
            return Ok(Some(BulkMessage::FileStart {
                id: self.plano.id,
                item: indice_no_fio(indice)?,
            }));
        }
    }

    /// Lê o próximo bloco do arquivo aberto, ou fecha-o com o resumo.
    async fn continuar(&mut self) -> Result<Option<BulkMessage>> {
        let Some(aberto) = self.aberto.as_mut() else {
            return Ok(None);
        };
        let mut bloco = vec![0u8; limits::MAX_FILE_BLOCK];
        let lidos = aberto
            .arquivo
            .read(&mut bloco)
            .await
            .map_err(|erro| FileError::io(&aberto.caminho, erro))?;

        if lidos == 0 {
            return self.fechar();
        }
        bloco.truncate(lidos);
        aberto.resumo.update(&bloco);
        let offset = aberto.lidos;
        let lidos64 = lidos as u64;
        aberto.lidos = aberto.lidos.saturating_add(lidos64);
        self.enviados = self.enviados.saturating_add(lidos64);
        Ok(Some(BulkMessage::FileBlock {
            id: self.plano.id,
            item: indice_no_fio(aberto.indice)?,
            offset,
            data: bloco,
        }))
    }

    /// Fecha o arquivo aberto com o `FileEnd`, conferindo o que o manifesto prometeu.
    fn fechar(&mut self) -> Result<Option<BulkMessage>> {
        let Some(aberto) = self.aberto.take() else {
            return Ok(None);
        };
        if aberto.lidos != aberto.declarado {
            return Err(FileError::MudouDurante {
                caminho: aberto.caminho,
                declarado: aberto.declarado,
                lidos: aberto.lidos,
            });
        }
        self.proximo = aberto.indice + 1;
        Ok(Some(BulkMessage::FileEnd {
            id: self.plano.id,
            item: indice_no_fio(aberto.indice)?,
            hash: *aberto.resumo.finalize().as_bytes(),
        }))
    }
}

/// O índice de um item, como ele viaja.
fn indice_no_fio(indice: usize) -> Result<u32> {
    u32::try_from(indice).map_err(|_| FileError::Violacao("índice de item não cabe no fio"))
}

/// O resumo BLAKE3 de um item do manifesto, lido do disco.
///
/// Usado pelos testes e pelo diagnóstico. Não é o caminho do envio: lá o resumo é calculado
/// **enquanto** os blocos saem, sem uma segunda leitura do arquivo.
///
/// # Errors
///
/// [`FileError::Io`] em falha de leitura.
pub async fn resumo_de(caminho: &std::path::Path) -> Result<[u8; 32]> {
    let mut arquivo = tokio::fs::File::open(caminho)
        .await
        .map_err(|erro| FileError::io(caminho, erro))?;
    let mut resumo = blake3::Hasher::new();
    let mut bloco = vec![0u8; limits::MAX_FILE_BLOCK];
    loop {
        let lidos = arquivo
            .read(&mut bloco)
            .await
            .map_err(|erro| FileError::io(caminho, erro))?;
        if lidos == 0 {
            return Ok(*resumo.finalize().as_bytes());
        }
        let Some(parte) = bloco.get(..lidos) else {
            return Err(FileError::Violacao("leitura maior que o buffer"));
        };
        resumo.update(parte);
    }
}

/// Se um item é arquivo com conteúdo a transportar.
#[must_use]
pub const fn tem_conteudo(item: &ManifestItem) -> bool {
    !item.is_dir
}
