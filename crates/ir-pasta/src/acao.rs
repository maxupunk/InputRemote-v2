//! O que o motor pede ao disco.
//!
//! O motor decide e o `ir-sincronia` executa, na ordem em que as ações vêm. Cada ação é pequena e
//! diz o que fazer, não como: criar as pastas de cima, conferir o resumo, gravar ao lado e renomear
//! no fim são do executor.

use ir_proto::message::EntryId;

/// Uma ação de disco, num caminho relativo à pasta compartilhada.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Acao {
    /// Buscar o conteúdo desta entrada na origem e pô-lo no caminho.
    Baixar(Baixar),
    /// Pôr no caminho uma entrada que ainda não foi baixada — o arquivo sob demanda do Windows.
    Marcador(Baixar),
    /// Copiar um arquivo daqui mesmo, que tem o conteúdo pedido: evita baixar o que já está no disco.
    Copiar {
        /// De onde.
        de: String,
        /// Para onde.
        para: String,
    },
    /// Renomear um arquivo daqui mesmo: o conteúdo mudou de nome do outro lado.
    Mover {
        /// De onde.
        de: String,
        /// Para onde.
        para: String,
    },
    /// Criar a subpasta.
    CriarPasta(String),
    /// Levar à lixeira da pasta. Nada é apagado de verdade pela sincronia.
    ParaLixeira(String),
    /// Na origem: pôr no caminho o arquivo que a réplica acabou de mandar.
    Publicar(String),
}

/// O que é preciso para buscar uma entrada.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Baixar {
    /// Onde ela fica.
    pub caminho: String,
    /// Qual é, na origem.
    pub entrada: EntryId,
    /// Em que versão.
    pub versao: u64,
    /// Quantos bytes.
    pub tamanho: u64,
    /// O resumo, para conferir no fim.
    pub resumo: Option<[u8; 32]>,
    /// A modificação na origem, para o arquivo daqui mostrar a mesma data.
    pub modificado_ns: i64,
}

impl Acao {
    /// O caminho em que a ação mexe por último — o que ela cria ou ocupa.
    #[must_use]
    pub fn caminho(&self) -> &str {
        match self {
            Self::Baixar(b) | Self::Marcador(b) => &b.caminho,
            Self::Copiar { para, .. } | Self::Mover { para, .. } => para,
            Self::CriarPasta(c) | Self::ParaLixeira(c) | Self::Publicar(c) => c,
        }
    }
}
