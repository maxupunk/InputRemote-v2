//! O tipo de erro do crate.
//!
//! A distinção que este módulo carrega, e que vale mais que os nomes: há **recusa** e há
//! **violação**.
//!
//! Uma recusa é uma resposta legítima a um pedido legítimo — não há cota, não há disco, o usuário
//! não autorizou. Ela vira uma [`RejectReason`](ir_proto::message::RejectReason) no fio, o par
//! entende, e o enlace continua de pé.
//!
//! Uma violação é o par dizendo algo que não pode ser verdade: um manifesto cujo total não bate
//! com a soma dos itens, um bloco de um item que não existe, um `FileEnd` de um arquivo que nunca
//! começou. Aí não há resposta cortês a dar — o codificador do outro lado está quebrado, e nada
//! do que ele disser depois merece confiança. O enlace cai
//! ([03, §8](../../../docs/03-protocolo.md)).

use std::path::PathBuf;

/// Resultado das operações de transferência.
pub type Result<T> = core::result::Result<T, FileError>;

/// O que pode dar errado numa transferência.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum FileError {
    /// Falha de E/S ao ler ou escrever.
    ///
    /// O caminho vai junto porque "erro de E/S" sozinho não diagnostica nada. Ele é registrado em
    /// `debug`, nunca acima ([04, §7](../../../docs/04-seguranca.md)).
    #[error("erro de E/S em {caminho}: {origem}")]
    Io {
        /// Onde.
        caminho: PathBuf,
        /// O quê.
        #[source]
        origem: std::io::Error,
    },

    /// O par disse algo que não pode ser verdade. O enlace cai.
    #[error("o par violou o protocolo: {0}")]
    Violacao(&'static str),

    /// O conteúdo chegou inteiro, mas o resumo não confere.
    ///
    /// É o que a transferência existe para detectar. Não há retentativa parcial: a árvore vai
    /// embora e a cópia é refeita.
    #[error("o resumo do item {item} não confere")]
    ResumoDivergente {
        /// Índice do item no manifesto.
        item: u32,
    },

    /// Um caminho local não pôde virar caminho relativo seguro para o manifesto.
    #[error("não consigo montar um caminho relativo para {0}")]
    CaminhoImpossivel(PathBuf),

    /// O que se pediu para enviar não existe, ou não é arquivo nem diretório.
    #[error("não sei enviar {0}")]
    NaoEnviavel(PathBuf),

    /// O arquivo mudou de tamanho entre o manifesto e o envio.
    ///
    /// Tem nome próprio porque, sem ele, o sintoma seria um "resumo divergente" no destino — que
    /// aponta para corrupção de transporte quando a causa foi o usuário salvando o arquivo no meio
    /// da cópia. Diagnóstico errado é pior que diagnóstico nenhum.
    #[error(
        "{caminho} mudou de tamanho durante o envio: {declarado} B declarados, {lidos} B lidos"
    )]
    MudouDurante {
        /// Qual arquivo.
        caminho: PathBuf,
        /// O que o manifesto disse.
        declarado: u64,
        /// O que se conseguiu ler.
        lidos: u64,
    },
}

impl FileError {
    /// Embrulha um erro de E/S com o caminho que o causou.
    pub fn io(caminho: impl Into<PathBuf>, origem: std::io::Error) -> Self {
        Self::Io {
            caminho: caminho.into(),
            origem,
        }
    }

    /// Se este erro obriga a derrubar o enlace.
    ///
    /// Só a violação de protocolo obriga. Falha de E/S e resumo divergente cancelam **a
    /// transferência**, e a sessão de entrada nem toma conhecimento
    /// ([01, §3.3](../../../docs/01-visao-e-escopo.md): falha de transferência não derruba nem
    /// atrasa a entrada).
    #[must_use]
    pub const fn derruba_o_enlace(&self) -> bool {
        matches!(self, Self::Violacao(_))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn so_a_violacao_de_protocolo_derruba_o_enlace() {
        assert!(FileError::Violacao("bloco sem item").derruba_o_enlace());

        let de_es = FileError::io(
            PathBuf::from("x"),
            std::io::Error::from(std::io::ErrorKind::PermissionDenied),
        );
        assert!(!de_es.derruba_o_enlace());
        assert!(!FileError::ResumoDivergente { item: 3 }.derruba_o_enlace());
        assert!(!FileError::NaoEnviavel(PathBuf::from("y")).derruba_o_enlace());
    }

    #[test]
    fn o_erro_de_es_diz_onde_aconteceu() {
        // "erro de E/S" sozinho não diagnostica nada, e este produto grava em disco como serviço
        // privilegiado — saber qual caminho falhou é a diferença entre um relato útil e um
        // inútil.
        let erro = FileError::io(
            PathBuf::from("recebidos/relatorio.pdf"),
            std::io::Error::from(std::io::ErrorKind::NotFound),
        );
        let texto = erro.to_string();
        assert!(texto.contains("recebidos"), "{texto}");
    }
}
