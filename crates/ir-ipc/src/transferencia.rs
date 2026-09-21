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
    /// cento. Dividir por zero seria o outro resultado.
    #[must_use]
    pub fn progresso(&self) -> f32 {
        if self.bytes_total == 0 {
            return 1.0;
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
        matches!(self.fase, Fase::Anunciada | Fase::Andando)
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
#[allow(clippy::float_cmp)]
mod tests {
    use super::*;

    fn copia(fase: Fase, sentido: Sentido, feitos: u64, total: u64) -> Transferencia {
        Transferencia {
            sentido,
            nome: "pasta-B".to_owned(),
            bytes_feitos: feitos,
            bytes_total: total,
            fase,
        }
    }

    #[test]
    fn o_andamento_diz_o_que_e_quanto() {
        let copia = copia(Fase::Andando, Sentido::Enviando, 512, 1024);
        assert_eq!(copia.titulo(), "Copiando para o outro computador");
        assert_eq!(copia.detalhe(), "pasta-B · 512 B de 1,0 KB · 50%");
        assert!(!copia.terminou() && !copia.falhou());
    }

    #[test]
    fn quem_recebe_sabe_onde_o_que_chegou_ficou() {
        let destino = if cfg!(windows) {
            r"C:\ProgramData\InputRemote\recebidos\pasta-B"
        } else {
            "/var/lib/inputremote/recebidos/pasta-B"
        };
        let copia = copia(
            Fase::Concluida {
                destino: destino.to_owned(),
            },
            Sentido::Recebendo,
            24,
            24,
        );
        assert_eq!(copia.titulo(), "Chegou: é só colar");
        assert!(
            copia.detalhe().starts_with("pasta-B · em "),
            "{}",
            copia.detalhe()
        );
        assert!(!copia.detalhe().ends_with("pasta-B"), "a pasta, não o item");
        assert!(copia.terminou() && !copia.falhou());
    }

    /// O defeito que motivou isto: a cópia parava, nada aparecia na tela, e colar do outro lado
    /// trazia a cópia **anterior** — que continuava no clipboard de lá.
    #[test]
    fn uma_copia_que_nao_atravessa_diz_por_que() {
        let copia = copia(Fase::Parada(Motivo::CanalCaiu), Sentido::Enviando, 0, 1024);
        assert_eq!(copia.titulo(), "A cópia não atravessou");
        assert!(
            copia.detalhe().contains("conexão de arquivos caiu"),
            "{}",
            copia.detalhe()
        );
        assert!(copia.terminou() && copia.falhou());
    }

    #[test]
    fn o_tamanho_e_legivel_em_cada_faixa() {
        assert_eq!(tamanho_legivel(0), "0 B");
        assert_eq!(tamanho_legivel(512), "512 B");
        assert_eq!(tamanho_legivel(1024), "1,0 KB");
        assert_eq!(tamanho_legivel(1_572_864), "1,5 MB");
        assert_eq!(tamanho_legivel(1_288_490_189), "1,2 GB");
        assert!(tamanho_legivel(u64::MAX).ends_with("GB"));
    }

    #[test]
    fn uma_arvore_so_de_pastas_esta_pronta_e_nao_em_zero_por_cento() {
        let copia = copia(Fase::Andando, Sentido::Enviando, 0, 0);
        assert!(copia.detalhe().ends_with("100%"), "{}", copia.detalhe());
    }

    fn andando(feitos: u64, total: u64) -> Transferencia {
        Transferencia {
            sentido: Sentido::Recebendo,
            nome: "relatório".to_owned(),
            bytes_feitos: feitos,
            bytes_total: total,
            fase: Fase::Andando,
        }
    }

    #[test]
    fn o_progresso_vai_de_zero_a_um() {
        assert_eq!(andando(0, 100).progresso(), 0.0);
        assert_eq!(andando(50, 100).progresso(), 0.5);
        assert_eq!(andando(100, 100).progresso(), 1.0);
    }

    #[test]
    fn uma_arvore_sem_bytes_esta_pronta_e_nao_em_zero_por_cento() {
        // Copiar uma estrutura de pastas vazia é legítimo, e a barra não pode dividir por zero
        // nem ficar parada no começo para sempre.
        assert_eq!(andando(0, 0).progresso(), 1.0);
    }

    #[test]
    fn um_progresso_maior_que_o_total_nao_passa_de_um() {
        // Defesa contra contagem errada: a barra pode estar errada, mas não pode estourar a tela.
        assert_eq!(andando(500, 100).progresso(), 1.0);
    }

    #[test]
    fn todo_motivo_tem_frase_em_portugues_e_nao_vazia() {
        let motivos = [
            Motivo::SemPermissao,
            Motivo::AcimaDaCota,
            Motivo::SemEspaco,
            Motivo::CaminhoInseguro,
            Motivo::ItensDemais,
            Motivo::ResumoDivergente,
            Motivo::Cancelada,
            Motivo::CanalCaiu,
            Motivo::Outro("o disco falhou".to_owned()),
        ];
        for motivo in motivos {
            let frase = motivo.descricao();
            assert!(!frase.is_empty(), "{motivo:?}");
            assert!(
                !frase.contains("Reason") && !frase.contains('_'),
                "`{frase}` parece nome de variante, não frase: a tela não traduz, ela mostra"
            );
        }
    }

    #[test]
    fn so_o_que_ainda_acontece_conta_como_em_curso() {
        let mut t = andando(1, 2);
        assert!(t.em_curso());
        t.fase = Fase::Anunciada;
        assert!(t.em_curso());
        t.fase = Fase::Concluida {
            destino: "C:/x".to_owned(),
        };
        assert!(!t.em_curso());
        t.fase = Fase::Parada(Motivo::Cancelada);
        assert!(!t.em_curso());
    }

    #[test]
    fn o_sentido_tem_rotulo() {
        assert_eq!(Sentido::Enviando.rotulo(), "enviando");
        assert_eq!(Sentido::Recebendo.rotulo(), "recebendo");
    }
}
