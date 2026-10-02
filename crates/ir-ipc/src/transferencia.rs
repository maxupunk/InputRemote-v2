//! O que a interface sabe sobre uma transferência de arquivos.
//!
//! Vocabulário próprio, e não os tipos de `ir-proto`. A regra é a de
//! [02, §2.1](../../../docs/02-arquitetura.md): se a tela desenhasse os tipos do fio, mudar o
//! formato de fio quebraria a interface — e a interface voltaria a ter opinião sobre protocolo.
//!
//! Aqui isso tem uma consequência concreta e boa: `RejectReason` e `CancelReason` viram um motivo
//! com frase pronta em português. A tela não traduz nada; ela mostra.

use serde::{Deserialize, Serialize};

/// Para que lado o conteúdo está indo.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Sentido {
    /// Deste computador para o outro.
    Enviando,
    /// Do outro para este.
    Recebendo,
}

impl Sentido {
    /// Uma palavra para a interface.
    #[must_use]
    pub const fn rotulo(self) -> &'static str {
        match self {
            Self::Enviando => "enviando",
            Self::Recebendo => "recebendo",
        }
    }
}

/// Por que uma transferência não aconteceu, ou parou.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum Motivo {
    /// O outro computador não aceita receber arquivos deste par.
    SemPermissao,
    /// Passa da cota configurada lá.
    AcimaDaCota,
    /// Não há espaço em disco no destino.
    SemEspaco,
    /// O manifesto trazia caminho que escaparia da pasta de destino.
    CaminhoInseguro,
    /// Itens demais numa transferência só.
    ItensDemais,
    /// O conteúdo chegou, mas o resumo não conferiu.
    ResumoDivergente,
    /// O usuário cancelou.
    Cancelada,
    /// O canal de dados caiu. A entrada **não** é afetada.
    CanalCaiu,
    /// Outra coisa, com a frase que o serviço tiver.
    Outro(String),
}

impl Motivo {
    /// A frase que a interface mostra.
    #[must_use]
    pub fn descricao(&self) -> String {
        match self {
            Self::SemPermissao => "o outro computador não aceita arquivos deste par".to_owned(),
            Self::AcimaDaCota => "passa do limite configurado no outro computador".to_owned(),
            Self::SemEspaco => "não há espaço em disco no outro computador".to_owned(),
            Self::CaminhoInseguro => "um dos caminhos não é seguro para o destino".to_owned(),
            Self::ItensDemais => "são arquivos demais numa transferência só".to_owned(),
            Self::ResumoDivergente => {
                "o conteúdo chegou diferente do que saiu; nada foi gravado".to_owned()
            }
            Self::Cancelada => "cancelada".to_owned(),
            Self::CanalCaiu => {
                "a conexão de arquivos caiu; teclado e mouse não foram afetados".to_owned()
            }
            Self::Outro(frase) => frase.clone(),
        }
    }
}

/// Em que pé está a transferência.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum Fase {
    /// Anunciada, esperando o outro lado aceitar.
    Anunciada,
    /// Em curso.
    Andando,
    /// Terminou, e o conteúdo está aqui.
    Concluida {
        /// Onde ficou, para a interface poder abrir a pasta.
        destino: String,
    },
    /// Não aconteceu, ou parou.
    Parada(Motivo),
    /// Parada à espera do canal de arquivos, que caiu ou ainda não voltou. Segue sozinha quando
    /// ele voltar; se ele demorar demais, vira [`Fase::Parada`] com [`Motivo::CanalCaiu`].
    ///
    /// Antes uma queda no meio era o fim da cópia, e quem copiou tinha de copiar de novo — mesmo
    /// quando a rede voltava em dois segundos.
    ///
    /// **No fim, e não ao lado de `Andando`.** O canal local é `postcard`, que numera as variantes
    /// pela posição. Inserida no meio, ela renumerou `Concluida`: o ajudante de clipboard que
    /// sobreviveu a uma atualização lia "concluída" como mensagem malformada e nunca punha no
    /// clipboard o que chegava — copiar do Linux para o Windows parou
    /// ([log 55](../../../docs/logs/55-o-manifesto-que-nao-cabia.md)).
    AguardandoConexao,
}

/// Uma transferência, como a interface a vê.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Transferencia {
    /// Para que lado.
    pub sentido: Sentido,
    /// O nome do que está indo — o da pasta ou do arquivo que o usuário copiou.
    ///
    /// Nome, e nunca caminho completo: caminho de arquivo em transferência é registrado em
    /// `debug` ([04, §7](../../../docs/04-seguranca.md)), e a interface não é `debug`.
    pub nome: String,
    /// Bytes já transferidos.
    pub bytes_feitos: u64,
    /// Bytes no total.
    pub bytes_total: u64,
    /// Em que pé está.
    pub fase: Fase,
}

impl Transferencia {
    /// O progresso de 0 a 1, para a barra.
    ///
    /// Uma transferência de zero byte — uma árvore só de pastas — está pronta, e não em zero por
    /// cento. Dividir por zero seria o outro resultado. A exceção é a cópia que espera a conexão
    /// sem nunca ter começado: o zero ali é "ainda não se sabe", e a barra cheia diria que chegou.
    #[must_use]
    pub fn progresso(&self) -> f32 {
        if self.bytes_total == 0 {
            return if self.fase == Fase::AguardandoConexao {
                0.0
            } else {
                1.0
            };
        }
        let feito = self.bytes_feitos.min(self.bytes_total);
        // A precisão de `f32` basta para uma barra: o erro máximo em 5 GB é de alguns bytes.
        #[allow(clippy::cast_precision_loss)]
        {
            feito as f32 / self.bytes_total as f32
        }
    }

    /// Se ela ainda está acontecendo.
    #[must_use]
    pub const fn em_curso(&self) -> bool {
        matches!(
            self.fase,
            Fase::Anunciada | Fase::Andando | Fase::AguardandoConexao
        )
    }
}

impl Transferencia {
    /// O que está acontecendo, em três ou quatro palavras.
    ///
    /// Título e detalhe existem aqui, e não na interface, porque **dois** programas os mostram: a
    /// janela (um cartão e um aviso no canto, no Windows) e o ajudante de clipboard (a notificação
    /// do sistema, no Linux). A mesma cópia dita com as mesmas palavras nos dois.
    #[must_use]
    pub fn titulo(&self) -> &'static str {
        match (&self.fase, self.sentido) {
            (Fase::Parada(_), _) => "A cópia não atravessou",
            (Fase::AguardandoConexao, _) => "Esperando a conexão voltar",
            (Fase::Concluida { .. }, Sentido::Enviando) => "Cópia entregue",
            (Fase::Concluida { .. }, Sentido::Recebendo) => "Chegou: é só colar",
            (Fase::Anunciada, Sentido::Enviando) => "Preparando a cópia",
            (Fase::Anunciada, Sentido::Recebendo) => "Chegando do outro computador",
            (_, Sentido::Enviando) => "Copiando para o outro computador",
            (_, Sentido::Recebendo) => "Recebendo do outro computador",
        }
    }

    /// O nome do que está indo, com o tamanho, o destino, ou o motivo de não ter ido.
    #[must_use]
    pub fn detalhe(&self) -> String {
        let nome = &self.nome;
        let tamanho = tamanho_legivel(self.bytes_total);
        match (&self.fase, self.sentido) {
            (Fase::Parada(motivo), _) => format!("{nome} · {}", motivo.descricao()),
            (Fase::AguardandoConexao, _) => format!(
                "{nome} · segue sozinha quando a conexão de arquivos voltar; teclado e mouse não foram afetados"
            ),
            (Fase::Concluida { .. }, Sentido::Enviando) => {
                format!("{nome} · {tamanho} — é só colar no outro computador")
            }
            (Fase::Concluida { destino }, Sentido::Recebendo) => {
                format!("{nome} · em {}", pasta_de(destino))
            }
            (Fase::Anunciada, _) => format!("{nome} · {tamanho}"),
            _ => format!(
                "{nome} · {} de {tamanho} · {}",
                tamanho_legivel(self.bytes_feitos),
                porcentagem(self.progresso())
            ),
        }
    }

    /// Se esta cópia terminou — deu certo ou não.
    #[must_use]
    pub const fn terminou(&self) -> bool {
        matches!(self.fase, Fase::Concluida { .. } | Fase::Parada(_))
    }

    /// Se ela terminou **mal**: é a única que pede ação de quem copiou.
    #[must_use]
    pub const fn falhou(&self) -> bool {
        matches!(self.fase, Fase::Parada(_))
    }
}

/// A porcentagem inteira, que é a precisão que serve para ler de relance.
fn porcentagem(progresso: f32) -> String {
    let inteiro = (progresso.clamp(0.0, 1.0) * 100.0).round();
    // O valor vem de `clamp`, então cabe em `i64` e não é NaN.
    #[allow(clippy::cast_possible_truncation)]
    {
        format!("{}%", inteiro as i64)
    }
}

/// O tamanho em unidade que se lê de relance: "1,2 GB", e não "1288490188 B".
///
/// Público porque a janela também o usa, para o tráfego da sessão: dois números do mesmo assunto
/// escritos de jeitos diferentes na mesma tela seriam dois assuntos.
#[must_use]
pub fn tamanho_legivel(bytes: u64) -> String {
    const PASSO: f64 = 1024.0;
    const UNIDADES: [&str; 4] = ["B", "KB", "MB", "GB"];
    #[allow(clippy::cast_precision_loss)]
    let mut valor = bytes as f64;
    let mut unidade = 0;
    while valor >= PASSO && unidade + 1 < UNIDADES.len() {
        valor /= PASSO;
        unidade += 1;
    }
    let nome = UNIDADES.get(unidade).copied().unwrap_or("B");
    if unidade == 0 {
        return format!("{bytes} {nome}");
    }
    // Vírgula decimal, que é como se escreve em português.
    format!("{valor:.1} {nome}").replace('.', ",")
}

/// A pasta onde o que chegou ficou, sem o nome do item — é o que o usuário precisa para achá-lo.
fn pasta_de(destino: &str) -> String {
    std::path::Path::new(destino)
        .parent()
        .map(|pasta| pasta.display().to_string())
        .filter(|texto| !texto.is_empty())
        .unwrap_or_else(|| destino.to_owned())
}

#[cfg(test)]
mod testes;
