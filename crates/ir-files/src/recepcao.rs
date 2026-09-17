//! O lado que recebe: onde a garantia de cópia é cobrada.
//!
//! Este é o módulo que decide o que vira arquivo no computador do usuário, e ele roda dentro de um
//! processo privilegiado a partir de bytes que vieram da rede. Por isso quase toda linha aqui é uma
//! verificação, e o padrão é **recusar**.
//!
//! # As garantias, e onde cada uma está
//!
//! | Garantia | Como |
//! |---|---|
//! | Nada é escrito fora do destino | [`Staging::caminho_de`] confere o caminho de novo, na hora de escrever |
//! | Nada além do que foi aceito | cada bloco é conferido contra o tamanho **declarado** do item |
//! | O conteúdo chegou íntegro | BLAKE3 por item, calculado enquanto se escreve |
//! | Nem árvore parcial nem arquivo temporário | a montagem se apaga no `Drop` |
//! | Publicação sem meio-caminho visível | um `rename` só, no fim |
//!
//! # Ignorar e derrubar são respostas diferentes
//!
//! Mensagem de **outra** transferência é ignorada: pode ser sobra de uma cópia que o usuário
//! substituiu, e o par tem o direito de ter mandado antes de saber. Mensagem **impossível** —
//! bloco de um item que não está aberto, `FileEnd` de arquivo incompleto, bloco que passa do
//! tamanho declarado — derruba o enlace, porque significa que o outro lado não está falando este
//! protocolo ([03, §8](../../../docs/03-protocolo.md)).

use std::path::{Path, PathBuf};

use ir_proto::message::{BulkMessage, CancelReason, ManifestItem, RejectReason, TransferId};
use tokio::io::AsyncWriteExt;
use tracing::debug;

use crate::cota::{Cota, EspacoLivre};
use crate::error::{FileError, Result};
use crate::publicacao::{Publicacao, como_publicar};
use crate::staging::Staging;

/// O arquivo que está sendo escrito agora.
#[derive(Debug)]
struct Aberto {
    item: u32,
    caminho: PathBuf,
    arquivo: tokio::fs::File,
    escritos: u64,
    declarado: u64,
    resumo: blake3::Hasher,
}

/// O resultado de tentar abrir uma recepção.
#[derive(Debug)]
pub enum Abertura {
    /// Aceita. Responda e siga.
    Aceita {
        /// O estado da recepção.
        recepcao: Box<Recepcao>,
        /// O `Accept` a mandar.
        resposta: BulkMessage,
    },
    /// Recusada, com motivo. Nada foi criado em disco.
    Recusada {
        /// O `Reject` a mandar.
        resposta: BulkMessage,
        /// Por quê, para a interface e o registro.
        motivo: RejectReason,
    },
}

/// O que fazer depois de aplicar uma mensagem.
#[derive(Debug)]
pub enum Reacao {
    /// Nada.
    Nada,
    /// Mandar isto ao par e continuar.
    Responder(BulkMessage),
    /// O último item conferiu: mandar isto e publicar com [`Recepcao::concluir`].
    Concluida(BulkMessage),
    /// O par desistiu. A montagem vai embora sozinha.
    Cancelada(CancelReason),
}

/// Recebe uma transferência, do manifesto à publicação.
#[derive(Debug)]
pub struct Recepcao {
    id: TransferId,
    itens: Vec<ManifestItem>,
    total: u64,
    nome: Publicacao,
    recebidos: PathBuf,
    staging: Staging,
    aberto: Option<Aberto>,
    escritos: u64,
    conferidos: usize,
    arquivos: usize,
}

impl Recepcao {
    /// Avalia um manifesto e, se ele passar, prepara a montagem.
    ///
    /// Os diretórios do manifesto são criados aqui, e não quando os arquivos chegam: se o disco vai
    /// recusar a árvore, é melhor descobrir antes de aceitar do que no meio.
    ///
    /// # Errors
    ///
    /// [`FileError::Violacao`] se o manifesto não pode ser verdade; [`FileError::Io`] em falha de
    /// escrita ao preparar a montagem.
    pub async fn abrir(
        recebidos: &Path,
        manifesto: (TransferId, Vec<ManifestItem>, u64),
        cota: Cota,
        livre: EspacoLivre,
    ) -> Result<Abertura> {
        let (id, itens, total) = manifesto;
        if let Some(motivo) = crate::cota::avaliar(&itens, total, cota, livre)? {
            return Ok(Abertura::Recusada {
                resposta: BulkMessage::Reject { id, reason: motivo },
                motivo,
            });
        }
        let staging = Staging::criar(recebidos, id).await?;
        for item in itens.iter().filter(|item| item.is_dir) {
            staging.criar_pasta(item).await?;
        }
        let arquivos = itens.iter().filter(|item| !item.is_dir).count();
        let nome = como_publicar(&itens);
        Ok(Abertura::Aceita {
            recepcao: Box::new(Self {
                id,
                itens,
                total,
                nome,
                recebidos: recebidos.to_path_buf(),
                staging,
                aberto: None,
                escritos: 0,
                conferidos: 0,
                arquivos,
            }),
            resposta: BulkMessage::Accept { id },
        })
    }

    /// Quantos bytes já foram escritos.
    #[must_use]
    pub const fn escritos(&self) -> u64 {
        self.escritos
    }

    /// Quantos bytes a transferência tem no total.
    #[must_use]
    pub const fn total(&self) -> u64 {
        self.total
    }

    /// Aplica uma mensagem do canal de dados.
    ///
    /// # Errors
    ///
    /// [`FileError::Violacao`] para mensagem impossível — quem chama derruba o enlace;
    /// [`FileError::ResumoDivergente`] se o conteúdo chegou mas não confere — quem chama manda
    /// `Cancel`; [`FileError::Io`] em falha de escrita.
    pub async fn aplicar(&mut self, mensagem: BulkMessage) -> Result<Reacao> {
        match mensagem {
            BulkMessage::FileStart { id, item } if id == self.id => self.abrir_item(item).await,
            BulkMessage::FileBlock {
                id,
                item,
                offset,
                data,
            } if id == self.id => self.escrever(item, offset, &data).await,
            BulkMessage::FileEnd { id, item, hash } if id == self.id => {
                self.fechar_item(item, hash).await
            }
            BulkMessage::Cancel { id, reason } if id == self.id => Ok(Reacao::Cancelada(reason)),
            // Um segundo manifesto, ou uma resposta que só quem envia recebe. Nada disso tem
            // lugar numa recepção aberta.
            BulkMessage::Manifest { id, .. }
            | BulkMessage::Accept { id }
            | BulkMessage::Reject { id, .. }
            | BulkMessage::Verified { id, .. }
                if id == self.id =>
            {
                Err(FileError::Violacao(
                    "mensagem fora de lugar no canal de dados",
                ))
            }
            // `Progress` é informativo, e mensagem de outra transferência é sobra de uma cópia que
            // o usuário já substituiu. Nem uma nem outra é motivo para derrubar nada.
            outra => {
                debug!(
                    ?outra,
                    "mensagem do canal de dados sem efeito nesta recepção"
                );
                Ok(Reacao::Nada)
            }
        }
    }

    /// Começa a receber um arquivo.
    async fn abrir_item(&mut self, indice: u32) -> Result<Reacao> {
        if self.aberto.is_some() {
            return Err(FileError::Violacao("abriu um item com outro ainda aberto"));
        }
        let item = self.item(indice)?.clone();
        if item.is_dir {
            return Err(FileError::Violacao(
                "um diretório não tem conteúdo a enviar",
            ));
        }
        let caminho = self.staging.preparar_pai(&item).await?;
        let arquivo = tokio::fs::File::create(&caminho)
            .await
            .map_err(|erro| FileError::io(&caminho, erro))?;
        self.aberto = Some(Aberto {
            item: indice,
            caminho,
            arquivo,
            escritos: 0,
            declarado: item.size,
            resumo: blake3::Hasher::new(),
        });
        Ok(Reacao::Nada)
    }

    /// Escreve um bloco, conferindo que ele pertence ao arquivo aberto e cabe nele.
    async fn escrever(&mut self, indice: u32, offset: u64, dados: &[u8]) -> Result<Reacao> {
        let aberto = self
            .aberto
            .as_mut()
            .ok_or(FileError::Violacao("bloco sem arquivo aberto"))?;
        if aberto.item != indice {
            return Err(FileError::Violacao("bloco de outro item"));
        }
        if aberto.escritos != offset {
            // Sobre TCP a ordem é garantida, então um deslocamento fora de lugar não é a rede
            // reordenando: é o par errado ou adulteração. Escrever em posição arbitrária deixaria
            // um buraco no arquivo e o resumo acusaria depois, sem dizer o porquê.
            return Err(FileError::Violacao("deslocamento fora de ordem"));
        }
        let cabe = u64::try_from(dados.len())
            .ok()
            .and_then(|tamanho| aberto.escritos.checked_add(tamanho))
            .is_some_and(|fim| fim <= aberto.declarado);
        if !cabe {
            // A cota foi aprovada para o total declarado. Sem esta linha, um par poderia anunciar
            // um byte e mandar gigabytes.
            return Err(FileError::Violacao("bloco passa do tamanho declarado"));
        }
        aberto
            .arquivo
            .write_all(dados)
            .await
            .map_err(|erro| FileError::io(&aberto.caminho, erro))?;
        aberto.resumo.update(dados);
        let tamanho = u64::try_from(dados.len()).unwrap_or(0);
        aberto.escritos = aberto.escritos.saturating_add(tamanho);
        self.escritos = self.escritos.saturating_add(tamanho);
        Ok(Reacao::Nada)
    }

    /// Fecha um arquivo e confere o resumo.
    async fn fechar_item(&mut self, indice: u32, resumo: [u8; 32]) -> Result<Reacao> {
        let mut aberto = self
            .aberto
            .take()
            .ok_or(FileError::Violacao("fim de um arquivo que não começou"))?;
        if aberto.item != indice {
            return Err(FileError::Violacao("fim de outro item"));
        }
        if aberto.escritos != aberto.declarado {
            return Err(FileError::Violacao(
                "o arquivo terminou antes do tamanho declarado",
            ));
        }
        aberto
            .arquivo
            .flush()
            .await
            .map_err(|erro| FileError::io(&aberto.caminho, erro))?;
        if *aberto.resumo.finalize().as_bytes() != resumo {
            return Err(FileError::ResumoDivergente { item: indice });
        }
        self.conferidos += 1;
        let resposta = BulkMessage::Verified {
            id: self.id,
            item: indice,
            ok: true,
        };
        if self.conferidos >= self.arquivos {
            Ok(Reacao::Concluida(resposta))
        } else {
            Ok(Reacao::Responder(resposta))
        }
    }

    /// Publica a árvore recebida e devolve onde ela ficou.
    ///
    /// # Errors
    ///
    /// [`FileError::Violacao`] se ainda falta item por conferir — publicar uma árvore incompleta
    /// seria entregar ao usuário algo que não é a cópia que ele pediu; [`FileError::Io`] se o
    /// `rename` falhar.
    pub async fn concluir(self) -> Result<PathBuf> {
        if self.conferidos < self.arquivos {
            return Err(FileError::Violacao("publicação com item por conferir"));
        }
        match &self.nome {
            Publicacao::Entrada(entrada) => {
                self.staging.publicar_dentro(&self.recebidos, entrada).await
            }
            Publicacao::Agrupadas(nome) => {
                let nome = nome.clone();
                self.staging.publicar(&self.recebidos, &nome).await
            }
        }
    }

    fn item(&self, indice: u32) -> Result<&ManifestItem> {
        usize::try_from(indice)
            .ok()
            .and_then(|indice| self.itens.get(indice))
            .ok_or(FileError::Violacao("item fora do manifesto"))
    }
}
