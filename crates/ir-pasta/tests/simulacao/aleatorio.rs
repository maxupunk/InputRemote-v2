//! Histórias sorteadas: edições, remoções e quedas do canal em ordem qualquer, dos dois lados.
//!
//! Sementes fixas, para uma falha ser reproduzível. Duas promessas conferidas no fim de cada uma:
//! as árvores ficam iguais, e nada que existia em algum dos lados antes de o canal voltar some — ou
//! está na árvore, ou numa lixeira.

use std::collections::BTreeSet;

use crate::bancada::{Bancada, Disco};

/// Um gerador pseudoaleatório mínimo (xorshift64*), para não depender de `rand` num crate puro.
struct Sorteio(u64);

impl Sorteio {
    fn proximo(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }

    fn ate(&mut self, limite: usize) -> usize {
        usize::try_from(self.proximo() % limite as u64).unwrap_or(0)
    }
}

/// Poucos caminhos, de propósito: com eles os dois lados mexem no mesmo arquivo o tempo todo.
const CAMINHOS: [&str; 6] = [
    "a.txt",
    "b.txt",
    "d/c.txt",
    "d/e/f.txt",
    "d/e/g.txt",
    "h.txt",
];

/// O que se pode apagar: os arquivos e as subpastas inteiras — apagar uma subpasta de um lado
/// enquanto o outro edita dentro dela é o caso que mais exige da regra "a edição vence".
fn removivel(sorteio: &mut Sorteio) -> &'static str {
    const SUBPASTAS: [&str; 2] = ["d", "d/e"];
    let indice = sorteio.ate(CAMINHOS.len() + SUBPASTAS.len());
    CAMINHOS
        .get(indice)
        .or_else(|| SUBPASTAS.get(indice - CAMINHOS.len()))
        .copied()
        .unwrap_or("a.txt")
}

/// Um passo sorteado. `com_canal`: se o passo pode ligar e desligar o canal.
fn passo(b: &mut Bancada, sorteio: &mut Sorteio, contador: &mut u32, com_canal: bool) {
    let caminho = CAMINHOS[sorteio.ate(CAMINHOS.len())];
    *contador += 1;
    let dados = format!("conteudo {contador}");
    let escolha = sorteio.ate(10);
    if std::env::var_os("IR_TRACO").is_some() {
        eprintln!("{contador}: {escolha} {caminho} ligado={}", b.ligado);
    }
    match escolha {
        0..=2 => b.grava_na_origem(caminho, dados.as_bytes()),
        3..=5 => b.grava_na_replica(caminho, dados.as_bytes()),
        6 => {
            b.disco_o.remover(removivel(sorteio));
        }
        7 => {
            b.disco_r.remover(removivel(sorteio));
        }
        8 if com_canal => b.ligado = !b.ligado,
        _ => b.rodar(),
    }
}

fn conteudos(disco: &Disco) -> BTreeSet<Vec<u8>> {
    let mut todos: BTreeSet<Vec<u8>> = disco.arquivos.values().map(|(d, _)| d.clone()).collect();
    todos.extend(disco.lixeira.iter().map(|(_, d)| d.clone()));
    todos
}

fn em_dia_ou_falha(b: &Bancada, semente: u64, fase: &str) {
    assert!(
        b.em_dia(),
        "semente {semente}, {fase}: as árvores divergiram
origem {:?}
réplica {:?}",
        b.disco_o.arvore(),
        b.disco_r.arvore()
    );
}

/// Uma história em duas fases.
///
/// Na primeira, tudo se mistura — edições, remoções, o canal caindo e voltando —, e a promessa é
/// convergir. Na segunda, os dois computadores ficam separados e cada um edita o seu; a promessa é
/// que nenhum conteúdo novo de nenhum dos dois lados se perca quando o canal voltar. Conteúdo que
/// já estava sincronizado pode ser substituído por uma edição posterior — é o que o usuário quis.
fn historia(semente: u64) {
    let mut b = Bancada::nova();
    let mut sorteio = Sorteio(semente.wrapping_mul(0x9e37_79b9_7f4a_7c15) | 1);
    let mut contador = 0;
    for _ in 0..60 {
        passo(&mut b, &mut sorteio, &mut contador, true);
    }
    b.ligado = true;
    b.rodar();
    em_dia_ou_falha(&b, semente, "primeira fase");
    let sincronizado = conteudos(&b.disco_o);

    b.ligado = false;
    for _ in 0..50 {
        passo(&mut b, &mut sorteio, &mut contador, false);
    }
    b.rodar();
    let novos: BTreeSet<Vec<u8>> = b
        .disco_o
        .arquivos
        .values()
        .chain(b.disco_r.arquivos.values())
        .map(|(d, _)| d.clone())
        .filter(|d| !sincronizado.contains(d))
        .collect();
    b.ligado = true;
    b.rodar();
    em_dia_ou_falha(&b, semente, "segunda fase");
    let mut depois = conteudos(&b.disco_o);
    depois.extend(conteudos(&b.disco_r));
    for dados in &novos {
        assert!(
            depois.contains(dados),
            "semente {semente}: {:?} sumiu",
            String::from_utf8_lossy(dados)
        );
    }
}

#[test]
fn trezentas_historias_sorteadas_terminam_em_dia_e_sem_perder_nada() {
    let so = std::env::var("IR_SEMENTE")
        .ok()
        .and_then(|s| s.parse().ok());
    for semente in so.map_or(1..=300, |s| s..=s) {
        historia(semente);
    }
}
