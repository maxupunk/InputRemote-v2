//! O conflito: quem fica com o nome, e como se chama a outra versão.
//!
//! A regra é a de Dropbox, OneDrive e Syncthing: **nada se perde**. Quando os dois lados mexeram no
//! mesmo arquivo — e quem descobre isso é a versão base, nunca o relógio —, as duas versões ficam.
//! A mais recente fica com o nome; a outra vira `nome (conflito NOTEBOOK 2026-10-02 14h30).ext`, ao
//! lado, onde o usuário a vê sem procurar.
//!
//! O relógio entra só para escolher quem fica com o nome, e mesmo aí corrigido pela diferença entre
//! as máquinas medida no `Hello`: um notebook com a hora errada não pode ganhar todos os conflitos.
//! O empate fica com a origem, para os dois lados chegarem à mesma decisão sem conversar.

/// Um dos dois lados da pasta.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lado {
    /// Quem compartilhou.
    Origem,
    /// Quem recebeu.
    Replica,
}

/// Qual versão fica com o nome num conflito.
///
/// `diferenca_ns` é quanto o relógio da réplica está adiantado em relação ao da origem — negativo
/// se atrasado —, como estimado no `Hello`. A modificação da réplica é trazida para o relógio da
/// origem antes de comparar. Empate fica com a origem.
#[must_use]
pub fn quem_fica_com_o_nome(origem_ns: i64, replica_ns: i64, diferenca_ns: i64) -> Lado {
    let replica_no_relogio_da_origem = replica_ns.saturating_sub(diferenca_ns);
    if replica_no_relogio_da_origem > origem_ns {
        Lado::Replica
    } else {
        Lado::Origem
    }
}

/// Um instante no calendário local, até o minuto — o que vai no nome da cópia de conflito.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Momento {
    /// Ano.
    pub ano: i64,
    /// Mês, de 1 a 12.
    pub mes: u8,
    /// Dia, de 1 a 31.
    pub dia: u8,
    /// Hora, de 0 a 23.
    pub hora: u8,
    /// Minuto, de 0 a 59.
    pub minuto: u8,
}

impl Momento {
    /// O instante `unix_ns` — nanossegundos desde 1970 em UTC — no fuso de quem vai ler o nome.
    ///
    /// O fuso vem de fora, em segundos a leste de UTC: este crate não pergunta nada ao sistema.
    #[must_use]
    pub fn de_unix_ns(unix_ns: i64, fuso_s: i32) -> Self {
        let segundos = unix_ns
            .div_euclid(1_000_000_000)
            .saturating_add(i64::from(fuso_s));
        let dias = segundos.div_euclid(86_400);
        let no_dia = segundos.rem_euclid(86_400);
        let (ano, mes, dia) = civil(dias);
        Self {
            ano,
            mes,
            dia,
            hora: u8::try_from(no_dia / 3_600).unwrap_or(0),
            minuto: u8::try_from(no_dia % 3_600 / 60).unwrap_or(0),
        }
    }
}

/// Ano, mês e dia de um dia contado desde 1970-01-01, no calendário gregoriano proléptico.
///
/// O algoritmo `civil_from_days` de Howard Hinnant (<https://howardhinnant.github.io/date_algorithms.html>):
/// conta em eras de 400 anos começando em março, o que põe o dia extra do ano bissexto no fim.
fn civil(dias: i64) -> (i64, u8, u8) {
    let z = dias.saturating_add(719_468);
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let dia = doy - (153 * mp + 2) / 5 + 1;
    let mes = if mp < 10 { mp + 3 } else { mp - 9 };
    let ano = yoe + era * 400 + i64::from(mes <= 2);
    (
        ano,
        u8::try_from(mes).unwrap_or(1),
        u8::try_from(dia).unwrap_or(1),
    )
}

/// O maior componente de caminho em NTFS e ext4, em bytes.
const MAX_COMPONENTE: usize = 255;

/// Quanto do nome da máquina entra na cópia de conflito.
const MAX_MAQUINA: usize = 32;

/// Quantas vezes tentar um nome livre antes de desistir de numerar.
const MAX_TENTATIVAS: u32 = 1_000;

/// O caminho da cópia de conflito de `caminho`, ao lado dele.
///
/// `maquina` é o computador onde a versão perdedora foi feita; `existe` responde se um caminho já
/// está ocupado na pasta, e um nome ocupado ganha um número no fim (`… 14h30 2)`).
///
/// O nome cabe sempre num componente de 255 bytes: o que encurta é o nome original, nunca a
/// etiqueta — sem ela o usuário não sabe de onde a cópia veio.
#[must_use]
pub fn nome_de_conflito(
    caminho: &str,
    maquina: &str,
    quando: Momento,
    existe: impl Fn(&str) -> bool,
) -> String {
    let (pasta, nome) = caminho
        .rsplit_once('/')
        .map_or(("", caminho), |(p, n)| (p, n));
    let (radical, extensao) = partir_extensao(nome);
    let maquina = limpar_maquina(maquina);
    let base = format!(
        "conflito {maquina} {:04}-{:02}-{:02} {:02}h{:02}",
        quando.ano, quando.mes, quando.dia, quando.hora, quando.minuto
    );

    let mut candidato = String::new();
    for tentativa in 1..=MAX_TENTATIVAS {
        let etiqueta = if tentativa == 1 {
            format!(" ({base})")
        } else {
            format!(" ({base} {tentativa})")
        };
        let componente = montar(radical, &etiqueta, extensao);
        candidato = if pasta.is_empty() {
            componente
        } else {
            format!("{pasta}/{componente}")
        };
        if !existe(&candidato) {
            break;
        }
    }
    candidato
}

/// Radical e extensão (com o ponto). Um arquivo que começa com ponto e não tem outro, como
/// `.bashrc`, não tem extensão: a etiqueta vai no fim.
fn partir_extensao(nome: &str) -> (&str, &str) {
    match nome.rfind('.') {
        Some(ponto) if ponto > 0 => nome.split_at(ponto),
        _ => (nome, ""),
    }
}

/// O radical encurtado até a etiqueta e a extensão caberem num componente.
fn montar(radical: &str, etiqueta: &str, extensao: &str) -> String {
    let (radical, extensao) = if etiqueta.len() + extensao.len() >= MAX_COMPONENTE {
        // Extensão absurda: vira parte do radical, que é o que encurta.
        ("", "")
    } else {
        (radical, extensao)
    };
    let cabe = MAX_COMPONENTE - etiqueta.len() - extensao.len();
    format!("{}{etiqueta}{extensao}", ate_o_limite(radical, cabe))
}

/// O maior prefixo de `texto` com no máximo `limite` bytes, sem partir um caractere.
fn ate_o_limite(texto: &str, limite: usize) -> &str {
    if texto.len() <= limite {
        return texto;
    }
    let mut fim = limite;
    while !texto.is_char_boundary(fim) {
        fim -= 1;
    }
    texto.get(..fim).unwrap_or("")
}

/// O nome da máquina que pode ir num nome de arquivo nos dois sistemas.
///
/// O nome vem do par — é o que ele diz de si — e vai parar no disco: o que o Windows interpreta em
/// vez de gravar vira hífen, e o tamanho é cortado.
fn limpar_maquina(maquina: &str) -> String {
    let limpo: String = maquina
        .chars()
        .map(|c| {
            let seguro = c.is_alphanumeric() || matches!(c, ' ' | '-' | '_' | '.');
            if seguro { c } else { '-' }
        })
        .collect();
    let aparar = |c: char| c == ' ' || c == '.';
    let limpo = ate_o_limite(limpo.trim_matches(aparar), MAX_MAQUINA).trim_matches(aparar);
    if limpo.is_empty() {
        "outro computador".to_owned()
    } else {
        limpo.to_owned()
    }
}

#[cfg(test)]
mod testes;
