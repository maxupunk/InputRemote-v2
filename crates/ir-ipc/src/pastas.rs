//! O canal do ajudante das pastas: o processo que cuida das pastas compartilhadas, como o usuário.
//!
//! Terceiro canal local, ao lado do de controle e do do agente
//! ([ADR-0015](../../../docs/adr/0015-pastas-compartilhadas.md)). Existe separado do de controle por
//! dois motivos que o código do serviço impõe:
//!
//! - O de controle atende um pedido por vez pelo ator, e descarta aviso quando quem lê se atrasa.
//!   Bloco de arquivo não pode ser descartado, e não pode esperar o ator: o serviço só **repassa**
//!   o que vai e vem do par, com fila limitada e contrapressão.
//! - Quem grava na pasta é o ajudante, e não o serviço: no Linux o serviço nem pode escrever em
//!   `$HOME` (`ProtectHome=read-only`), e no Windows gravaria como `SYSTEM` na pasta do usuário.
//!
//! As mensagens do par viajam aqui com o tipo do protocolo ([`FolderMessage`]), como no canal do
//! agente: o ajudante é quem fala com o par, e o serviço não abre nenhuma delas. O que a janela vê
//! tem vocabulário próprio — [`ResumoDePasta`] e [`ComandoDePasta`] —, para `ir-ui` continuar sem
//! opinião sobre o formato de fio.
//!
//! Regra de compatibilidade de todo canal local: variante nova entra **no fim** do enum
//! ([log 56](../../../docs/logs/56-o-ajudante-que-sobreviveu.md)). O teste
//! `o_numero_de_cada_variante_no_fio_nao_muda` guarda isso.

use ir_proto::message::FolderMessage;
use serde::{Deserialize, Serialize};

/// Uma mensagem da pasta, como vai e vem do par. O tipo do protocolo, com o nome deste canal: quem
/// só repassa — o serviço — não precisa de uma seta para `ir-proto` para nomeá-lo.
pub use ir_proto::message::FolderMessage as MensagemDoPar;

/// Identificador de uma pasta compartilhada, no vocabulário da janela.
///
/// Os mesmos dezesseis bytes do protocolo, em tipo próprio pelo mesmo motivo de
/// [`crate::vocabulario`]: a janela não depende de `ir-proto`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct IdDePasta(pub [u8; 16]);

/// O que o ajudante das pastas diz ao serviço.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum DoAjudanteDePastas {
    /// O primeiro que o ajudante diz: as pastas de que cuida.
    ///
    /// O serviço associa cada pasta ao usuário da conexão e recusa outro usuário que a reivindique
    /// depois — duas sessões abertas na mesma máquina não leem a pasta uma da outra.
    Apresentar {
        /// As pastas.
        pastas: Vec<IdDePasta>,
    },
    /// Uma mensagem para o par, que o serviço repassa sem abrir.
    ParaOPar(FolderMessage),
    /// Como as pastas estão, para a janela. Substitui o resumo anterior inteiro.
    Resumo(Vec<ResumoDePasta>),
    /// Um pedido da janela não deu certo: a frase, com o que fazer, para ela mostrar.
    Recado(String),
    /// O outro computador copiou (Ctrl+C) arquivos de uma pasta compartilhada: estes caminhos, na
    /// cópia daqui da mesma pasta, vão para o clipboard deste computador.
    PorNoClipboard(Vec<String>),
}

/// O que o serviço diz ao ajudante das pastas.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum ParaOAjudanteDePastas {
    /// Uma mensagem que veio do par.
    DoPar(FolderMessage),
    /// Como está o canal de dados com o par. Vem na conexão e a cada mudança.
    Enlace {
        /// Se o canal de dados está de pé agora.
        de_pe: bool,
        /// Se o par negociou uma versão que conhece pastas. Falso com o canal de pé quer dizer
        /// "atualize o outro computador".
        par_suporta: bool,
        /// O nome do outro computador, para as frases e para a cópia de conflito.
        nome_do_par: String,
    },
    /// Um pedido da janela, repassado.
    Comando(ComandoDePasta),
}

/// O que a janela pode pedir sobre as pastas.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum ComandoDePasta {
    /// Compartilhar uma pasta que já existe, onde ela está.
    Compartilhar {
        /// O caminho completo da pasta.
        caminho: String,
    },
    /// Criar uma pasta nova, já compartilhada, no lugar padrão.
    Criar {
        /// O nome dela.
        nome: String,
    },
    /// Aceitar a pasta que o outro computador ofereceu.
    Aceitar(IdDePasta),
    /// Recusar a pasta que o outro computador ofereceu.
    Recusar(IdDePasta),
    /// Parar de compartilhar. Nada é apagado: cada lado fica com o que tem no disco.
    Parar(IdDePasta),
    /// Abrir a pasta no gerenciador de arquivos.
    Abrir(IdDePasta),
    /// Resolver um conflito.
    Resolver {
        /// A pasta.
        pasta: IdDePasta,
        /// O caminho do arquivo, relativo à pasta.
        caminho: String,
        /// O que fazer.
        escolha: EscolhaDeConflito,
    },
    /// Abrir a lixeira da pasta, onde fica o que a sincronia tirou nos últimos 30 dias.
    AbrirLixeira(IdDePasta),
    /// A pessoa copiou (Ctrl+C) estes arquivos da pasta: vão ao outro computador como caminhos da
    /// pasta, e não pela cópia de arquivos. Quem manda é o ajudante de clipboard.
    Copiado {
        /// A pasta.
        pasta: IdDePasta,
        /// Os caminhos, relativos à pasta, com `/`.
        caminhos: Vec<String>,
    },
}

/// O que fazer com as duas versões de um arquivo em conflito.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum EscolhaDeConflito {
    /// Manter as duas, como já estão. Só some o aviso.
    ManterAsDuas,
    /// Ficar com a que tem o nome; a cópia de conflito vai para a lixeira da pasta.
    FicarComEsta,
    /// Ficar com a cópia de conflito, que volta a ter o nome; a outra vai para a lixeira.
    FicarComAOutra,
}

/// Uma pasta, como a janela a mostra.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResumoDePasta {
    /// A pasta.
    pub id: IdDePasta,
    /// O nome.
    pub nome: String,
    /// Onde ela fica neste computador. Vazio enquanto uma oferta não for aceita.
    pub caminho_local: String,
    /// Se este computador compartilhou ou recebeu.
    pub papel: PapelDaPasta,
    /// Como ela está.
    pub situacao: SituacaoDaPasta,
    /// Quantas mudanças deste lado esperam para ir ao outro.
    pub pendentes: u32,
    /// Quantos conflitos esperam o usuário olhar.
    pub conflitos: u32,
    /// Quantos arquivos estão vindo do outro computador agora.
    pub baixando: u32,
    /// Os conflitos, um por arquivo, para a tela oferecer a escolha.
    pub lista_de_conflitos: Vec<ConflitoDePasta>,
}

/// Um arquivo que foi mudado nos dois computadores ao mesmo tempo: as duas versões estão guardadas.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConflitoDePasta {
    /// O caminho, relativo à pasta, da versão que ficou com o nome — a mais recente.
    pub original: String,
    /// O caminho da outra versão, a cópia de conflito.
    pub copia: String,
}

/// Se este computador compartilhou a pasta ou a recebeu.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PapelDaPasta {
    /// Este computador compartilhou; os arquivos de verdade estão aqui.
    Compartilhada,
    /// O outro computador compartilhou com este.
    Recebida,
}

/// Como uma pasta está agora.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum SituacaoDaPasta {
    /// Os dois lados iguais.
    EmDia,
    /// Mandando ou recebendo mudanças.
    Sincronizando,
    /// O outro computador não está ao alcance; as mudanças daqui esperam e vão sozinhas.
    SemConexao,
    /// O outro computador tem uma versão que não conhece pastas.
    ParDesatualizado,
    /// O outro computador ofereceu esta pasta e espera a resposta.
    Oferecida,
}

mod frases;

pub use frases::{frase_do_conflito, resumo_das_pastas};

#[cfg(test)]
mod testes;
