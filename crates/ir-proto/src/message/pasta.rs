//! Canal 5 — a pasta compartilhada ([ADR-0015](../../../../docs/adr/0015-pastas-compartilhadas.md)).
//!
//! Uma pasta tem **origem**, o computador que a compartilhou e guarda os arquivos de verdade, e
//! **réplica**, o outro, que vê a árvore inteira e baixa o conteúdo quando precisa. A origem é o
//! sequenciador: cada mudança aceita, dela ou vinda da réplica, ganha o próximo número da pasta, e
//! esse número é a versão da entrada. É a versão — nunca o relógio — que diz se houve conflito: a
//! réplica manda junto de cada mudança a versão em que se baseou (`base`), e uma base que não é mais
//! a atual quer dizer que os dois lados mexeram no mesmo arquivo.
//!
//! Estas mensagens viajam no canal 5 dentro de [`BulkMessage::Folder`](super::BulkMessage::Folder),
//! mas **não** passam pela fila das cópias do clipboard: uma cópia nova cancela a anterior, e a
//! sincronia não pode cancelar o Ctrl+C do usuário nem ser cancelada por ele.
//!
//! Só saem para um par que negociou a versão 8 ou mais ([`crate::version::supports_folders`]):
//! mensagem desconhecida derruba o enlace (`docs/03-protocolo.md` §8), e um par da versão 7
//! derrubaria junto a cópia do clipboard.

use serde::{Deserialize, Serialize};

/// Identificador de uma pasta compartilhada, sorteado por quem compartilha.
///
/// Dezesseis bytes aleatórios, e não o nome: o nome é do usuário, pode repetir e pode mudar.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct FolderId(pub [u8; 16]);

/// Identificador de uma entrada — arquivo ou subpasta — dentro de uma pasta.
///
/// Dado pela origem e nunca reaproveitado, nem depois de a entrada ser apagada: um pedido de
/// conteúdo atrasado nunca recebe os bytes de outro arquivo.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct EntryId(pub u64);

/// Identificador de uma operação da réplica — envio, apagar, criar ou renomear —, para casar com
/// o [`FolderMessage::Outcome`] que a responde.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct OpId(pub u32);

/// Identificador de um pedido de trecho, para casar com os [`FolderMessage::Range`] e para
/// cancelar.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct RangeId(pub u32);

/// O papel de quem fala, nesta pasta.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Role {
    /// Compartilhou a pasta e guarda os arquivos de verdade.
    Origin,
    /// Vê a pasta do outro.
    Replica,
}

/// Se a entrada é arquivo ou subpasta.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EntryKind {
    /// Arquivo comum. Ligação simbólica e ponto de nova análise não viajam.
    File,
    /// Subpasta.
    Directory,
}

/// Uma entrada do índice da pasta, como a origem a conhece.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    /// O identificador, estável enquanto a entrada existir.
    pub id: EntryId,
    /// Caminho relativo à raiz da pasta, sempre com `/`. Validado por
    /// [`crate::message::data::is_safe_relative_path`].
    pub path: String,
    /// Arquivo ou subpasta.
    pub kind: EntryKind,
    /// Tamanho em bytes. Zero para subpasta.
    pub size: u64,
    /// Última modificação, em nanossegundos desde 1970 no relógio da origem.
    ///
    /// Só escolhe qual versão fica com o nome num conflito, e nunca decide se houve conflito.
    pub modified_ns: i64,
    /// BLAKE3 do conteúdo, quando a origem já o calculou.
    ///
    /// Opcional para uma pasta grande aparecer na réplica antes de a origem terminar de ler tudo.
    pub hash: Option<[u8; 32]>,
    /// A versão: o número da pasta quando esta entrada mudou pela última vez.
    pub version: u64,
    /// Lápide: a entrada foi apagada nesta versão.
    pub deleted: bool,
}

/// Uma pasta que quem fala conhece, anunciada no [`FolderMessage::Hello`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KnownFolder {
    /// A pasta.
    pub folder: FolderId,
    /// O papel de quem fala.
    pub role: Role,
    /// Na origem, o número atual da pasta; na réplica, o último que ela aplicou.
    pub seen_up_to: u64,
}

/// Mensagem da pasta compartilhada.
///
/// A ordem das variantes é o formato de fio: variante nova entra **no fim**, sempre.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum FolderMessage {
    /// O primeiro que cada lado diz quando o canal sobe: as pastas que conhece e o relógio.
    ///
    /// O relógio serve para estimar a diferença entre as máquinas, que normaliza o horário de
    /// modificação antes de comparar num conflito.
    Hello {
        /// As pastas, com o papel e até onde cada uma foi vista.
        folders: Vec<KnownFolder>,
        /// O relógio de quem fala, em nanossegundos desde 1970.
        clock_ns: i64,
        /// Se este `Hello` responde a um do par. Um que não responde pede resposta; uma resposta
        /// não pede nada — é o que deixa um lado que reiniciou o ajudante ser ouvido sem os dois
        /// ficarem se respondendo para sempre.
        reply: bool,
    },
    /// Dito pelo serviço, e não pelo ajudante: não há ninguém deste lado cuidando das pastas
    /// agora — a sessão do usuário está fechada, ou o ajudante caiu.
    HelperAbsent,
    /// A origem oferece uma pasta.
    Offer {
        /// A pasta.
        folder: FolderId,
        /// O nome que a pasta tem na origem. Um componente de caminho só.
        name: String,
        /// Quantas entradas ela tem agora.
        entries: u32,
        /// A soma dos tamanhos, para a réplica conferir o disco antes de aceitar.
        total_bytes: u64,
    },
    /// A réplica aceita a pasta.
    Accept {
        /// A pasta.
        folder: FolderId,
    },
    /// A réplica recusa a pasta.
    Decline {
        /// A pasta.
        folder: FolderId,
        /// Por quê.
        reason: DeclineFolder,
    },
    /// Qualquer um dos lados para de compartilhar. Ninguém apaga nada: cada lado fica com o que já
    /// tem no disco.
    Stop {
        /// A pasta.
        folder: FolderId,
    },
    /// A réplica pede as mudanças desde um número da pasta. Zero pede tudo.
    RequestChanges {
        /// A pasta.
        folder: FolderId,
        /// O último número que a réplica já aplicou.
        since: u64,
    },
    /// Mudanças do índice, em ordem de versão. Vem como resposta ou espontânea, quando a pasta
    /// muda na origem; muitas mudanças vêm em várias mensagens ([`changes_messages`]).
    Changes {
        /// A pasta.
        folder: FolderId,
        /// As entradas que mudaram.
        entries: Vec<Entry>,
        /// O número da pasta que esta leva alcança, quando aplicada inteira.
        up_to: u64,
        /// Se é a última mensagem desta leva.
        last: bool,
    },
    /// A réplica confirma que aplicou até este número. A origem pode esquecer as lápides abaixo.
    Acknowledge {
        /// A pasta.
        folder: FolderId,
        /// Até onde.
        seq: u64,
    },
    /// Pede um trecho do conteúdo de uma entrada, numa versão.
    ///
    /// Pedir por trecho é o que dá retomada de graça depois de uma queda, e é exatamente a forma do
    /// pedido do Windows quando um arquivo sob demanda é aberto.
    RequestRange {
        /// A pasta.
        folder: FolderId,
        /// Este pedido.
        request: RangeId,
        /// A entrada.
        entry: EntryId,
        /// A versão esperada. Se a origem já tiver outra, responde [`RangeFailure::Stale`].
        version: u64,
        /// De onde.
        offset: u64,
        /// Quantos bytes, no máximo [`crate::limits::MAX_RANGE_REQUEST`].
        len: u32,
    },
    /// Um pedaço do trecho pedido, de no máximo [`crate::limits::MAX_FILE_BLOCK`] bytes.
    Range {
        /// A pasta.
        folder: FolderId,
        /// O pedido.
        request: RangeId,
        /// De onde, dentro do arquivo.
        offset: u64,
        /// Os bytes.
        data: Vec<u8>,
    },
    /// O trecho não pôde ser servido.
    RangeFailed {
        /// A pasta.
        folder: FolderId,
        /// O pedido.
        request: RangeId,
        /// Por quê.
        reason: RangeFailure,
    },
    /// Quem pediu desistiu do trecho.
    CancelRange {
        /// A pasta.
        folder: FolderId,
        /// O pedido.
        request: RangeId,
    },
    /// A réplica quer gravar um arquivo na origem. Os bytes só vão depois do crédito.
    Upload {
        /// A pasta.
        folder: FolderId,
        /// Esta operação.
        op: OpId,
        /// O caminho.
        path: String,
        /// A versão em que a réplica se baseou. Zero para um arquivo novo.
        base: u64,
        /// O tamanho.
        size: u64,
        /// O BLAKE3 do conteúdo. Se a origem já tem esses bytes, responde [`FolderMessage::AlreadyHave`].
        hash: [u8; 32],
        /// A modificação, no relógio da réplica.
        modified_ns: i64,
    },
    /// A origem libera mais bytes deste envio.
    Credit {
        /// A pasta.
        folder: FolderId,
        /// O envio.
        op: OpId,
        /// Quantos bytes a mais podem vir.
        bytes: u32,
    },
    /// A origem já tem esse conteúdo — um arquivo renomeado, ou copiado dentro da pasta — e o
    /// grava sem receber os bytes de novo.
    AlreadyHave {
        /// A pasta.
        folder: FolderId,
        /// O envio.
        op: OpId,
    },
    /// Um bloco do envio, de no máximo [`crate::limits::MAX_FILE_BLOCK`] bytes.
    UploadBlock {
        /// A pasta.
        folder: FolderId,
        /// O envio.
        op: OpId,
        /// De onde, dentro do arquivo.
        offset: u64,
        /// Os bytes.
        data: Vec<u8>,
    },
    /// Todos os bytes foram. A origem confere o resumo e responde com o [`FolderMessage::Outcome`].
    UploadEnd {
        /// A pasta.
        folder: FolderId,
        /// O envio.
        op: OpId,
    },
    /// A réplica apagou uma entrada.
    Delete {
        /// A pasta.
        folder: FolderId,
        /// Esta operação.
        op: OpId,
        /// O caminho.
        path: String,
        /// A versão que a réplica apagou. Se a origem tiver uma mais nova, a edição vence.
        base: u64,
    },
    /// A réplica criou uma subpasta.
    CreateDirectory {
        /// A pasta.
        folder: FolderId,
        /// Esta operação.
        op: OpId,
        /// O caminho.
        path: String,
    },
    /// A réplica renomeou ou moveu uma entrada.
    Rename {
        /// A pasta.
        folder: FolderId,
        /// Esta operação.
        op: OpId,
        /// O caminho de antes.
        from: String,
        /// O caminho de agora.
        to: String,
        /// A versão que a réplica renomeou.
        base: u64,
    },
    /// O que a origem fez com uma operação da réplica.
    Outcome {
        /// A pasta.
        folder: FolderId,
        /// A operação.
        op: OpId,
        /// O desfecho.
        result: OpResult,
    },
    /// A pessoa copiou (Ctrl+C) arquivos desta pasta. O outro computador põe no clipboard dele os
    /// mesmos caminhos, na cópia dele da pasta: colar lá cola o que a pasta já tem, e o conteúdo não
    /// atravessa pela cópia de arquivos — quando precisa atravessar, é pela pasta, uma vez só.
    Copied {
        /// A pasta.
        folder: FolderId,
        /// Os caminhos copiados, relativos à pasta, com `/`.
        paths: Vec<String>,
    },
}

impl FolderMessage {
    /// A pasta de que a mensagem fala; `None` nas de sessão, que falam de todas.
    ///
    /// É por ela que o serviço entrega cada mensagem ao ajudante do usuário dono da pasta.
    #[must_use]
    pub const fn folder(&self) -> Option<FolderId> {
        match self {
            Self::Hello { .. } | Self::HelperAbsent => None,
            Self::Offer { folder, .. }
            | Self::Accept { folder }
            | Self::Decline { folder, .. }
            | Self::Stop { folder }
            | Self::RequestChanges { folder, .. }
            | Self::Changes { folder, .. }
            | Self::Acknowledge { folder, .. }
            | Self::RequestRange { folder, .. }
            | Self::Range { folder, .. }
            | Self::RangeFailed { folder, .. }
            | Self::CancelRange { folder, .. }
            | Self::Upload { folder, .. }
            | Self::Credit { folder, .. }
            | Self::AlreadyHave { folder, .. }
            | Self::UploadBlock { folder, .. }
            | Self::UploadEnd { folder, .. }
            | Self::Delete { folder, .. }
            | Self::CreateDirectory { folder, .. }
            | Self::Rename { folder, .. }
            | Self::Outcome { folder, .. }
            | Self::Copied { folder, .. } => Some(*folder),
        }
    }
}

/// O desfecho de uma operação da réplica.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum OpResult {
    /// Aplicada, e a entrada ficou nesta versão.
    Accepted {
        /// A versão nova.
        version: u64,
    },
    /// Os dois lados mexeram no mesmo arquivo. As duas versões ficaram: a mais recente com o nome,
    /// a outra como cópia de conflito.
    Conflict {
        /// A versão da pasta depois de guardar as duas.
        version: u64,
        /// Onde ficou a cópia de conflito.
        conflict_path: String,
    },
    /// A réplica apagou o que a origem tinha editado: a edição vence, e a entrada volta.
    Resurrected {
        /// A versão que voltou.
        version: u64,
    },
    /// Não foi aplicada.
    Refused(Refusal),
}

mod motivos;
mod paginas;
mod validacao;

pub use motivos::{DeclineFolder, RangeFailure, Refusal};

pub use paginas::changes_messages;
pub use validacao::validate_folder_message;

#[cfg(test)]
mod testes;
