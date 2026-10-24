//! Os detalhes prontos para desenhar: os cards de processo, a lista de processos e a cópia.
//!
//! O card de processo e o da cópia são [`Bloco`]s como os outros: o aplicativo os desenha, move,
//! destaca e tira com o mesmo código. A lista é uma tabela de [`LinhaDeProcesso`], que o aplicativo
//! só ordena pela coluna escolhida.

use serde::Serialize;

use super::{Bloco, SEM_DADO, decimal, kib, porcento, taxa, vazio};
use crate::detalhes::{Copia, FaseDaCopia};
use crate::nivel::{self, Nivel};
use crate::processos::UsoDeProcesso;

/// O começo da chave de um card de processo: `proc:docker`.
pub const PREFIXO_DE_PROCESSO: &str = "proc:";

/// A chave do card da cópia de arquivos.
pub const CHAVE_DA_COPIA: &str = "copia";

/// A chave do card que acompanha os processos deste nome.
#[must_use]
pub fn chave_do_processo(nome: &str) -> String {
    format!("{PREFIXO_DE_PROCESSO}{}", nome.to_lowercase())
}

/// Uma linha da lista de processos.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct LinhaDeProcesso {
    /// A chave do card que acompanharia este processo — o botão "acompanhar" da linha a usa pronta.
    pub chave: String,
    /// O nome do executável.
    pub nome: String,
    /// Quantos processos com este nome.
    pub instancias: u16,
    /// "12,3 %".
    pub cpu: String,
    /// "1,2 GB".
    pub memoria: String,
    /// Para ordenar pelo processador.
    pub cpu_permille: u16,
    /// Para ordenar pela memória.
    pub memoria_kib: u64,
    /// A cor do processador.
    pub nivel: Nivel,
}

/// O card de um processo vigiado.
pub(super) fn vigiado(uso: &UsoDeProcesso) -> Bloco {
    let chave = chave_do_processo(&uso.nome);
    if uso.instancias == 0 {
        return Bloco {
            detalhe: Some("não está rodando".to_owned()),
            ..vazio(&chave, &uso.nome)
        };
    }
    let quantos = if uso.instancias == 1 {
        "1 processo".to_owned()
    } else {
        format!("{} processos", uso.instancias)
    };
    Bloco {
        valor: porcento(uso.cpu_permille),
        detalhe: Some(format!("{} · {quantos}", kib(uso.memoria_kib))),
        fracao: Some(f32::from(uso.cpu_permille.min(1000)) / 1000.0),
        nivel: nivel::do_processador(Some(uso.cpu_permille)),
        serie: Some(f32::from(uso.cpu_permille) / 10.0),
        ..vazio(&chave, &uso.nome)
    }
}

/// Uma linha da lista.
pub(super) fn linha(uso: &UsoDeProcesso) -> LinhaDeProcesso {
    LinhaDeProcesso {
        chave: chave_do_processo(&uso.nome),
        nome: uso.nome.clone(),
        instancias: uso.instancias,
        cpu: format!("{} %", decimal(f64::from(uso.cpu_permille) / 10.0, 1)),
        memoria: kib(uso.memoria_kib),
        cpu_permille: uso.cpu_permille,
        memoria_kib: uso.memoria_kib,
        nivel: nivel::do_processador(Some(uso.cpu_permille)),
    }
}

/// O card da cópia: quanto foi, o quê, a que velocidade, e quanto falta.
pub(super) fn copia(copia: &Copia) -> Bloco {
    let rotulo = if copia.enviando {
        "Cópia · enviando"
    } else {
        "Cópia · recebendo"
    };
    let fracao = progresso(copia);
    let (valor, nivel, situacao) = match &copia.fase {
        FaseDaCopia::Concluida => ("concluída".to_owned(), Nivel::Normal, None),
        FaseDaCopia::Parada(motivo) => ("parada".to_owned(), Nivel::Critico, Some(motivo.clone())),
        FaseDaCopia::AguardandoConexao => (
            porcento_de(fracao),
            Nivel::Atencao,
            Some("esperando a conexão".to_owned()),
        ),
        FaseDaCopia::Esperando => (
            SEM_DADO.to_owned(),
            Nivel::Normal,
            Some("esperando o outro lado".to_owned()),
        ),
        FaseDaCopia::Andando => (porcento_de(fracao), Nivel::Normal, andamento(copia)),
    };
    // A situação antes do nome: num card estreito, quem perde letras é o nome, e não a velocidade.
    let detalhe = situacao
        .into_iter()
        .chain(std::iter::once(copia.nome.clone()))
        .collect::<Vec<_>>()
        .join(" · ");
    #[allow(clippy::cast_precision_loss)]
    let serie = copia
        .por_segundo
        .filter(|_| copia.em_curso())
        .map(|b| b as f32 / 1_048_576.0);
    Bloco {
        valor,
        detalhe: Some(detalhe),
        fracao: Some(fracao),
        nivel,
        serie,
        ..vazio(CHAVE_DA_COPIA, rotulo)
    }
}

/// De 0 a 1. Zero byte é uma árvore só de pastas: pronta, e não em zero por cento.
fn progresso(copia: &Copia) -> f32 {
    if copia.total == 0 {
        return if copia.em_curso() { 0.0 } else { 1.0 };
    }
    #[allow(clippy::cast_precision_loss)]
    let fracao = copia.feitos.min(copia.total) as f32 / copia.total as f32;
    fracao
}

fn porcento_de(fracao: f32) -> String {
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let milesimos = (fracao.clamp(0.0, 1.0) * 1000.0).round() as u16;
    porcento(milesimos)
}

/// "12,3 MB/s · faltam ~30 s", quando já há velocidade.
fn andamento(copia: &Copia) -> Option<String> {
    let velocidade = copia.por_segundo.filter(|v| *v > 0)?;
    let resta = copia.total.saturating_sub(copia.feitos) / velocidade;
    Some(format!("{} · faltam {}", taxa(velocidade), tempo(resta)))
}

/// Segundos em "~30 s", "~2 min", "~1 h": é estimativa, e o til diz isso.
fn tempo(segundos: u64) -> String {
    match segundos {
        0..60 => format!("~{} s", segundos.max(1)),
        60..3600 => format!("~{} min", segundos.div_ceil(60)),
        _ => format!("~{} h", segundos.div_ceil(3600)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn uso(instancias: u16, cpu: u16) -> UsoDeProcesso {
        UsoDeProcesso {
            nome: "Docker".to_owned(),
            instancias,
            cpu_permille: cpu,
            memoria_kib: 2 * 1024 * 1024,
        }
    }

    fn uma_copia(fase: FaseDaCopia, feitos: u64, por_segundo: Option<u64>) -> Copia {
        Copia {
            enviando: true,
            nome: "fotos".to_owned(),
            feitos,
            total: 100 * 1_048_576,
            por_segundo,
            fase,
        }
    }

    #[test]
    fn o_card_do_processo_diz_quanto_usa_e_quantos_sao() {
        let bloco = vigiado(&uso(3, 123));
        assert_eq!(bloco.chave, "proc:docker");
        assert_eq!(bloco.rotulo, "Docker");
        assert_eq!(bloco.valor, "12 %");
        assert_eq!(bloco.detalhe.as_deref(), Some("2,0 GB · 3 processos"));
        let parado = vigiado(&uso(0, 0));
        assert_eq!(parado.valor, SEM_DADO);
        assert_eq!(parado.nivel, Nivel::SemDado);
        assert_eq!(parado.detalhe.as_deref(), Some("não está rodando"));
    }

    #[test]
    fn a_linha_traz_a_chave_do_card_e_os_numeros_para_ordenar() {
        let linha = linha(&uso(1, 456));
        assert_eq!(linha.chave, "proc:docker");
        assert_eq!(linha.cpu, "45,6 %");
        assert_eq!(linha.memoria_kib, 2 * 1024 * 1024);
    }

    #[test]
    fn a_copia_andando_diz_velocidade_e_quanto_falta() {
        let bloco = copia(&uma_copia(
            FaseDaCopia::Andando,
            40 * 1_048_576,
            Some(2 * 1_048_576),
        ));
        assert_eq!(bloco.chave, CHAVE_DA_COPIA);
        assert_eq!(bloco.valor, "40 %");
        let detalhe = bloco.detalhe.unwrap();
        assert!(detalhe.ends_with(" · fotos"), "{detalhe}");
        assert!(detalhe.contains("faltam ~30 s"), "{detalhe}");
        assert_eq!(bloco.serie, Some(2.0));
    }

    #[test]
    fn a_copia_parada_e_vermelha_e_diz_por_que() {
        let parada = copia(&uma_copia(
            FaseDaCopia::Parada("sem espaço".to_owned()),
            10,
            None,
        ));
        assert_eq!(parada.nivel, Nivel::Critico);
        assert_eq!(parada.detalhe.as_deref(), Some("sem espaço · fotos"));
        let pronta = copia(&uma_copia(FaseDaCopia::Concluida, 100 * 1_048_576, Some(1)));
        assert_eq!(pronta.valor, "concluída");
        assert_eq!(pronta.serie, None, "no fim não há velocidade");
    }

    #[test]
    fn o_tempo_e_estimativa() {
        assert_eq!(tempo(0), "~1 s");
        assert_eq!(tempo(90), "~2 min");
        assert_eq!(tempo(7200), "~2 h");
    }
}
