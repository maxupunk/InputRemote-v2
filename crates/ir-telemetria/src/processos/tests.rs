use std::sync::{Arc, Mutex};

use super::*;

/// Devolve as fotos dadas, uma por medida.
struct Roteiro(Arc<Mutex<Vec<Foto>>>);

impl Fotografo for Roteiro {
    fn fotografar(&mut self) -> Option<Foto> {
        let mut fotos = self.0.lock().ok()?;
        (!fotos.is_empty()).then(|| fotos.remove(0))
    }
}

fn bruto(pid: u32, nome: &str, tempo: u64, memoria_kib: u64) -> Bruto {
    Bruto {
        pid,
        nome: nome.to_owned(),
        tempo,
        memoria_kib,
    }
}

fn medidor(fotos: Vec<Foto>) -> Medidor {
    Medidor::novo(Roteiro(Arc::new(Mutex::new(fotos))))
}

/// A segunda medida: a primeira só guarda a foto.
fn segunda(fotos: Vec<Foto>, interesse: &InteresseEmProcessos) -> Option<Processos> {
    let mut medidor = medidor(fotos);
    assert_eq!(medidor.medir(interesse), None);
    medidor.medir(interesse)
}

fn interesse(nomes: &[&str], lista: bool) -> InteresseEmProcessos {
    InteresseEmProcessos {
        nomes: nomes.iter().map(|n| (*n).to_owned()).collect(),
        lista,
    }
}

#[test]
fn a_primeira_medida_nao_inventa_uso() {
    let foto = Foto {
        total: 1000,
        processos: vec![bruto(1, "docker", 500, 10)],
    };
    assert_eq!(
        medidor(vec![foto]).medir(&interesse(&["docker"], false)),
        None
    );
}

#[test]
fn o_vigiado_soma_quem_casa_com_o_nome() {
    let antes = Foto {
        total: 1000,
        processos: vec![
            bruto(1, "com.docker.backend", 100, 0),
            bruto(2, "VmmemWSL", 0, 0),
        ],
    };
    let depois = Foto {
        total: 2000,
        processos: vec![
            bruto(1, "com.docker.backend", 200, 1024),
            bruto(2, "VmmemWSL", 300, 4096),
            // Nasceu no intervalo: tudo o que gastou foi nele.
            bruto(3, "docker", 50, 512),
        ],
    };
    let medido = segunda(
        vec![antes, depois],
        &interesse(&["docker", "wsl", "steam"], false),
    )
    .unwrap();
    let docker = &medido.vigiados[0];
    assert_eq!(
        (docker.instancias, docker.cpu_permille, docker.memoria_kib),
        (2, 150, 1536)
    );
    assert_eq!(medido.vigiados[1].cpu_permille, 300);
    assert_eq!(medido.vigiados[2].instancias, 0, "não está rodando");
    assert!(medido.lista.is_empty(), "a lista não foi pedida");
}

#[test]
fn a_lista_junta_os_pesados_em_processador_e_em_memoria() {
    let processos: Vec<Bruto> = (0..20u32)
        .map(|i| bruto(i, &format!("p{i:02}"), u64::from(i), u64::from(20 - i)))
        .collect();
    let antes = Foto {
        total: 0,
        processos: processos
            .iter()
            .map(|p| Bruto {
                tempo: 0,
                ..p.clone()
            })
            .collect(),
    };
    let depois = Foto {
        total: 1000,
        processos,
    };
    let lista = segunda(vec![antes, depois], &interesse(&[], true))
        .unwrap()
        .lista;
    assert_eq!(lista.len(), MAX_LISTA);
    assert_eq!(
        lista[0].nome, "p19",
        "o mais pesado em processador vem primeiro"
    );
    assert!(
        lista.iter().any(|g| g.nome == "p00"),
        "o mais pesado em memória entra"
    );
}

#[test]
fn processos_com_o_mesmo_nome_viram_um_grupo() {
    let antes = Foto {
        total: 0,
        processos: vec![bruto(1, "chrome", 0, 0), bruto(2, "chrome", 0, 0)],
    };
    let depois = Foto {
        total: 100,
        processos: vec![bruto(1, "chrome", 10, 100), bruto(2, "chrome", 20, 200)],
    };
    let lista = segunda(vec![antes, depois], &interesse(&[], true))
        .unwrap()
        .lista;
    assert_eq!(lista.len(), 1);
    assert_eq!(
        (
            lista[0].instancias,
            lista[0].cpu_permille,
            lista[0].memoria_kib
        ),
        (2, 300, 300)
    );
}

#[test]
fn esquecer_recomeca_a_conta() {
    let foto = |total| Foto {
        total,
        processos: Vec::new(),
    };
    let mut medidor = medidor(vec![foto(0), foto(10), foto(20)]);
    let tudo = interesse(&[], true);
    assert_eq!(medidor.medir(&tudo), None);
    medidor.esquecer();
    assert_eq!(
        medidor.medir(&tudo),
        None,
        "depois de esquecer, de novo sem diferença"
    );
    assert!(medidor.medir(&tudo).is_some());
}
