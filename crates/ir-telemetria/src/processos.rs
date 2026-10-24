//! Os processos desta máquina, agrupados pelo nome, com processador e memória.
//!
//! O sistema dá uma [`Foto`]: cada processo com o tempo de processador gasto desde que nasceu. O
//! uso é a diferença entre duas fotos, dividida pelo tempo da máquina inteira no mesmo intervalo —
//! a mesma conta do processador da amostra ([`crate::conta`]). Esta parte é pura; ler a foto é
//! de cada sistema.
//!
//! Só mede com interesse: sem card de processo nem lista aberta, nenhuma foto é tirada.

use std::collections::HashMap;

use ir_monitor::processos::{InteresseEmProcessos, MAX_LISTA, Processos, UsoDeProcesso, casa};

/// Quantos de cada ranking entram na lista: os mais pesados em processador e em memória.
const DE_CADA: usize = MAX_LISTA / 2;

/// Um processo neste instante.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bruto {
    /// O identificador do sistema.
    pub pid: u32,
    /// O nome do executável, sem extensão.
    pub nome: String,
    /// O tempo de processador gasto desde que nasceu, na unidade de [`Foto::total`].
    pub tempo: u64,
    /// A memória residente, em KiB.
    pub memoria_kib: u64,
}

/// Os processos da máquina num instante, com o tempo total do processador até ali.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Foto {
    /// O tempo de todos os núcleos somados, ocioso inclusive.
    pub total: u64,
    /// Os processos que o sistema deixou ler.
    pub processos: Vec<Bruto>,
}

/// Quem tira a foto: um por sistema.
pub trait Fotografo: Send {
    /// A foto de agora; `None` se o sistema não deixou.
    fn fotografar(&mut self) -> Option<Foto>;
}

/// Mede os processos entre duas fotos.
pub struct Medidor {
    fotografo: Box<dyn Fotografo>,
    anterior: Option<(u64, HashMap<u32, u64>)>,
}

impl core::fmt::Debug for Medidor {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Medidor").finish_non_exhaustive()
    }
}

impl Medidor {
    /// Um medidor com este fotógrafo.
    #[must_use]
    pub fn novo(fotografo: impl Fotografo + 'static) -> Self {
        Self {
            fotografo: Box::new(fotografo),
            anterior: None,
        }
    }

    /// Os processos desde a medida anterior, do jeito que o interesse pede. `None` na primeira
    /// medida — sem duas fotos não há uso a calcular — e quando o sistema não deixou ler.
    pub fn medir(&mut self, interesse: &InteresseEmProcessos) -> Option<Processos> {
        let foto = self.fotografo.fotografar()?;
        let tempos = foto.processos.iter().map(|p| (p.pid, p.tempo)).collect();
        let anterior = self.anterior.replace((foto.total, tempos))?;
        let grupos = agrupar(&foto, &anterior);
        Some(escolher(&grupos, interesse))
    }

    /// Esquece a foto anterior: o próximo interesse recomeça, sem uma diferença de minutos atrás.
    pub fn esquecer(&mut self) {
        self.anterior = None;
    }
}

/// Os processos juntos pelo nome, com o uso desde a foto anterior em milésimos da máquina.
fn agrupar(foto: &Foto, (total_antes, antes): &(u64, HashMap<u32, u64>)) -> Vec<UsoDeProcesso> {
    let intervalo = foto.total.saturating_sub(*total_antes);
    let mut grupos: HashMap<&str, UsoDeProcesso> = HashMap::new();
    for processo in &foto.processos {
        // Quem nasceu no intervalo gastou nele tudo o que gastou.
        let gasto = processo
            .tempo
            .saturating_sub(antes.get(&processo.pid).copied().unwrap_or(0));
        let grupo = grupos
            .entry(processo.nome.as_str())
            .or_insert_with(|| UsoDeProcesso {
                nome: processo.nome.clone(),
                instancias: 0,
                cpu_permille: 0,
                memoria_kib: 0,
            });
        grupo.instancias = grupo.instancias.saturating_add(1);
        grupo.cpu_permille = grupo
            .cpu_permille
            .saturating_add(milesimos(gasto, intervalo));
        grupo.memoria_kib = grupo.memoria_kib.saturating_add(processo.memoria_kib);
    }
    grupos.into_values().collect()
}

fn milesimos(gasto: u64, intervalo: u64) -> u16 {
    if intervalo == 0 {
        return 0;
    }
    let milesimos = u128::from(gasto) * 1000 / u128::from(intervalo);
    u16::try_from(milesimos.min(1000)).unwrap_or(1000)
}

/// O que o interesse pede: um uso por nome vigiado e, se pedida, a lista dos mais pesados.
fn escolher(grupos: &[UsoDeProcesso], interesse: &InteresseEmProcessos) -> Processos {
    let vigiados = interesse
        .nomes
        .iter()
        .map(|nome| somar(grupos, nome))
        .collect();
    let lista = if interesse.lista {
        mais_pesados(grupos)
    } else {
        Vec::new()
    };
    Processos { vigiados, lista }.nos_tetos()
}

/// Todos os grupos que casam com o nome, somados sob o nome pedido.
fn somar(grupos: &[UsoDeProcesso], nome: &str) -> UsoDeProcesso {
    grupos.iter().filter(|g| casa(&g.nome, nome)).fold(
        UsoDeProcesso {
            nome: nome.to_owned(),
            instancias: 0,
            cpu_permille: 0,
            memoria_kib: 0,
        },
        |soma, g| UsoDeProcesso {
            instancias: soma.instancias.saturating_add(g.instancias),
            cpu_permille: soma.cpu_permille.saturating_add(g.cpu_permille).min(1000),
            memoria_kib: soma.memoria_kib.saturating_add(g.memoria_kib),
            ..soma
        },
    )
}

/// Os mais pesados em processador somados aos mais pesados em memória, sem repetir, do maior
/// processador para o menor. A tela reordena pela coluna que a pessoa escolher.
fn mais_pesados(grupos: &[UsoDeProcesso]) -> Vec<UsoDeProcesso> {
    let mut por_cpu: Vec<&UsoDeProcesso> = grupos.iter().collect();
    por_cpu.sort_by(|a, b| {
        b.cpu_permille
            .cmp(&a.cpu_permille)
            .then(a.nome.cmp(&b.nome))
    });
    let mut por_memoria = por_cpu.clone();
    por_memoria.sort_by(|a, b| b.memoria_kib.cmp(&a.memoria_kib).then(a.nome.cmp(&b.nome)));
    let mut lista: Vec<UsoDeProcesso> = Vec::with_capacity(MAX_LISTA);
    for grupo in por_cpu
        .iter()
        .take(DE_CADA)
        .chain(por_memoria.iter().take(DE_CADA))
    {
        if !lista.iter().any(|g| g.nome == grupo.nome) {
            lista.push((*grupo).clone());
        }
    }
    lista.sort_by(|a, b| {
        b.cpu_permille
            .cmp(&a.cpu_permille)
            .then(b.memoria_kib.cmp(&a.memoria_kib))
    });
    lista
}

#[cfg(test)]
mod tests;
