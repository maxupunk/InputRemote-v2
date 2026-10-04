//! O sistema de arquivos de verdade, montado numa pasta temporária.
//!
//! Pula sozinho onde não há FUSE (`/dev/fuse` e o `fusermount3`): num contêiner sem o dispositivo,
//! ou num CI sem o pacote. Na bancada roda num Fedora com `--device /dev/fuse`.

use std::path::Path;
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use super::*;

struct Bancada {
    base: std::path::PathBuf,
    conteudo: std::path::PathBuf,
    ponto: std::path::PathBuf,
    montagem: Arc<Montagem>,
    pedidos: Receiver<Pedido>,
}

impl Drop for Bancada {
    fn drop(&mut self) {
        let _ = std::process::Command::new("fusermount3")
            .arg("-u")
            .arg(&self.ponto)
            .output();
        let _ = std::fs::remove_dir_all(&self.base);
    }
}

fn montada(nome: &str) -> Option<Bancada> {
    if !suportado(Path::new("/")) {
        eprintln!("sem FUSE nesta máquina; teste pulado");
        return None;
    }
    let base = std::env::temp_dir().join(format!("ir-fuse-{nome}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let conteudo = base.join("cache");
    let ponto = base.join("ponto");
    std::fs::create_dir_all(conteudo.join(".inputremote")).unwrap();
    std::fs::create_dir_all(conteudo.join("sub")).unwrap();
    let (envia, pedidos) = mpsc::channel();
    let envia = Mutex::new(envia);
    let montagem = montar(&ponto, &conteudo, move |pedido| {
        let _ = envia.lock().unwrap().send(pedido);
    })
    .unwrap();
    Some(Bancada {
        base,
        conteudo,
        ponto,
        montagem: Arc::new(montagem),
        pedidos,
    })
}

/// Um arquivo do cache sem conteúdo: esparso, com o tamanho dado, e marcado.
fn sem(bancada: &Bancada, relativo: &str, tamanho: u64) {
    let caminho = bancada.conteudo.join(relativo);
    std::fs::File::create(&caminho)
        .unwrap()
        .set_len(tamanho)
        .unwrap();
    marcar_sem_conteudo(&caminho, true).unwrap();
}

/// Responde aos pedidos da montagem como o ajudante, até `leitor` terminar: um trecho sai de
/// `conteudo`; o arquivo inteiro é gravado no cache e posto no lugar. Devolve quantos de cada.
fn atender(
    b: &Bancada,
    relativo: &str,
    conteudo: &[u8],
    leitor: &std::thread::JoinHandle<()>,
) -> (u32, u32) {
    let (mut trechos, mut inteiros) = (0, 0);
    while !leitor.is_finished() {
        let Ok(Pedido::Buscar(busca)) = b.pedidos.recv_timeout(Duration::from_millis(50)) else {
            continue;
        };
        assert_eq!(busca.caminho, relativo);
        if busca.tamanho == u64::MAX {
            inteiros += 1;
            let montado = b.conteudo.join(".inputremote/novo");
            std::fs::write(&montado, conteudo).unwrap();
            std::fs::rename(&montado, b.conteudo.join(relativo)).unwrap();
            b.montagem.pronto(relativo, true);
        } else {
            trechos += 1;
            let inicio = usize::try_from(busca.offset).unwrap();
            let fim = inicio + usize::try_from(busca.tamanho).unwrap();
            entregar(
                0,
                busca.transferencia,
                busca.offset,
                conteudo.get(inicio..fim).unwrap(),
            )
            .unwrap();
        }
    }
    (trechos, inteiros)
}

#[test]
fn ler_so_o_comeco_traz_so_o_trecho_e_o_arquivo_continua_sem_conteudo() {
    let Some(b) = montada("comeco") else { return };
    let conteudo: Vec<u8> = (0..400_000u32).map(|i| (i % 241) as u8).collect();
    sem(&b, "sub/foto.png", 400_000);
    let nomes: Vec<String> = std::fs::read_dir(&b.ponto)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(nomes, vec!["sub"], "a pasta de controle não aparece");
    let alvo = b.ponto.join("sub/foto.png");
    assert_eq!(
        std::fs::metadata(&alvo).unwrap().len(),
        400_000,
        "o tamanho certo sem o conteúdo"
    );

    // Como a detecção de tipo: abrir e ler os primeiros 4 KiB.
    let lido = Arc::new(Mutex::new(Vec::new()));
    let guardar = Arc::clone(&lido);
    let leitor = std::thread::spawn(move || {
        let mut cabeca = vec![0u8; 4096];
        std::io::Read::read_exact(&mut std::fs::File::open(alvo).unwrap(), &mut cabeca).unwrap();
        *guardar.lock().unwrap() = cabeca;
    });
    let (trechos, inteiros) = atender(&b, "sub/foto.png", &conteudo, &leitor);
    assert_eq!(*lido.lock().unwrap(), conteudo.get(..4096).unwrap());
    assert!(
        trechos >= 1 && inteiros == 0,
        "só trechos: {trechos} trechos, {inteiros} inteiros"
    );
    assert!(
        sem_conteudo(&b.conteudo.join("sub/foto.png")),
        "o arquivo continua sem conteúdo"
    );
}

#[test]
fn ler_um_arquivo_pequeno_ate_o_fim_o_traz_para_o_disco() {
    let Some(b) = montada("pequeno") else { return };
    sem(&b, "a.txt", 11);
    let alvo = b.ponto.join("a.txt");
    let lido = Arc::new(Mutex::new(Vec::new()));
    let guardar = Arc::clone(&lido);
    let leitor = std::thread::spawn(move || {
        *guardar.lock().unwrap() = std::fs::read(alvo).unwrap();
    });
    let (_, inteiros) = atender(&b, "a.txt", b"onze bytes!", &leitor);
    assert_eq!(*lido.lock().unwrap(), b"onze bytes!");
    assert_eq!(inteiros, 1);
    assert!(
        !sem_conteudo(&b.conteudo.join("a.txt")),
        "ficou no disco, para abrir sem rede"
    );
}

#[test]
fn ler_alem_do_comeco_traz_o_arquivo_inteiro_mesmo_com_outro_tamanho() {
    let Some(b) = montada("inteiro") else { return };
    // A origem mudou depois do marcador: o conteúdo tem um byte a mais.
    let conteudo: Vec<u8> = (0..600_001u32).map(|i| (i % 251) as u8).collect();
    sem(&b, "grande.bin", 600_000);
    let alvo = b.ponto.join("grande.bin");
    let lido = Arc::new(Mutex::new(Vec::new()));
    let guardar = Arc::clone(&lido);
    let leitor = std::thread::spawn(move || {
        *guardar.lock().unwrap() = std::fs::read(alvo).unwrap();
    });
    let (_, inteiros) = atender(&b, "grande.bin", &conteudo, &leitor);
    assert_eq!(inteiros, 1);
    assert_eq!(
        *lido.lock().unwrap(),
        conteudo,
        "inteiro, com o tamanho novo"
    );
    assert!(
        !sem_conteudo(&b.conteudo.join("grande.bin")),
        "a marca saiu com o download"
    );
}

#[test]
fn escrever_criar_e_renomear_pela_montagem_chegam_ao_cache() {
    let Some(b) = montada("escrever") else { return };
    std::fs::write(b.ponto.join("novo.txt"), b"feito aqui").unwrap();
    assert_eq!(
        std::fs::read(b.conteudo.join("novo.txt")).unwrap(),
        b"feito aqui"
    );
    std::fs::rename(b.ponto.join("novo.txt"), b.ponto.join("sub/movido.txt")).unwrap();
    assert!(b.conteudo.join("sub/movido.txt").exists());
    std::fs::create_dir(b.ponto.join("pasta-nova")).unwrap();
    assert!(b.conteudo.join("pasta-nova").is_dir());
    std::fs::remove_file(b.ponto.join("sub/movido.txt")).unwrap();
    assert!(!b.conteudo.join("sub/movido.txt").exists());
}

/// Responde como o ajudante sem o outro computador, até `leitor` terminar: nenhum pedido vem. O
/// núcleo repete uma leitura que falhou, então pode haver mais de um.
fn recusar_tudo<T>(b: &Bancada, relativo: &str, leitor: &std::thread::JoinHandle<T>) {
    while !leitor.is_finished() {
        let Ok(Pedido::Buscar(busca)) = b.pedidos.recv_timeout(Duration::from_millis(50)) else {
            continue;
        };
        if busca.tamanho == u64::MAX {
            b.montagem.pronto(relativo, false);
        } else {
            falhar(0, busca.transferencia, (busca.offset, busca.tamanho)).unwrap();
        }
    }
}

#[test]
fn sem_o_outro_computador_ler_falha_com_rede_inalcancavel() {
    let Some(b) = montada("longe") else { return };
    let inicio = std::time::Instant::now();
    for (relativo, tamanho) in [("longe.txt", 5), ("longe.bin", 600_000)] {
        sem(&b, relativo, tamanho);
        let alvo = b.ponto.join(relativo);
        let leitor = std::thread::spawn(move || std::fs::read(alvo));
        recusar_tudo(&b, relativo, &leitor);
        let erro = leitor.join().unwrap().unwrap_err();
        assert_eq!(
            erro.raw_os_error(),
            Some(libc::ENETUNREACH),
            "{relativo}: {erro}"
        );
    }
    assert!(
        inicio.elapsed() < Duration::from_secs(10),
        "falhar é na hora, sem esperar prazo"
    );
}

#[test]
fn offline_abrir_falha_na_hora_e_gravar_por_cima_funciona() {
    let Some(b) = montada("por-cima") else { return };
    sem(&b, "notas.txt", 40);
    b.montagem.alcance(false);
    let erro = std::fs::File::open(b.ponto.join("notas.txt")).unwrap_err();
    assert_eq!(erro.raw_os_error(), Some(libc::ENETUNREACH), "{erro}");
    // Gravar por cima (o `>` do terminal) não precisa do conteúdo antigo.
    std::fs::write(b.ponto.join("notas.txt"), b"escrito offline").unwrap();
    assert!(
        b.pedidos.try_recv().is_err(),
        "nada foi pedido ao outro computador"
    );
    assert_eq!(
        std::fs::read(b.ponto.join("notas.txt")).unwrap(),
        b"escrito offline"
    );
    assert!(!sem_conteudo(&b.conteudo.join("notas.txt")));
}

/// Um observador inotify na pasta, como o do Nautilus (o `GFileMonitor` da `GLib`).
fn observar(pasta: &Path) -> i32 {
    use std::os::unix::ffi::OsStrExt;
    let alvo = std::ffi::CString::new(pasta.as_os_str().as_bytes()).unwrap();
    // SAFETY: sem argumentos de memória.
    let fd = unsafe { libc::inotify_init1(libc::IN_NONBLOCK) };
    assert!(fd >= 0);
    let mascara = libc::IN_CREATE | libc::IN_MOVED_TO | libc::IN_MOVED_FROM | libc::IN_DELETE;
    // SAFETY: o caminho termina em zero e vive até o fim da chamada.
    let observacao = unsafe { libc::inotify_add_watch(fd, alvo.as_ptr(), mascara) };
    assert!(observacao >= 0);
    fd
}

/// Os eventos que chegaram ao observador em até `prazo`: a máscara e o nome.
fn eventos(fd: i32, prazo: Duration) -> Vec<(u32, String)> {
    let fim = std::time::Instant::now() + prazo;
    let mut vistos = Vec::new();
    let mut buffer = vec![0u8; 8192];
    while std::time::Instant::now() < fim {
        // SAFETY: o buffer é local e o tamanho é o dele.
        let lidos = unsafe { libc::read(fd, buffer.as_mut_ptr().cast(), buffer.len()) };
        let Ok(lidos) = usize::try_from(lidos) else {
            std::thread::sleep(Duration::from_millis(20));
            continue;
        };
        let mut i = 0;
        while i + 16 <= lidos {
            let campo =
                |j: usize| u32::from_ne_bytes(buffer.get(j..j + 4).unwrap().try_into().unwrap());
            let (mascara, tamanho) = (campo(i + 4), campo(i + 12) as usize);
            let nome = buffer.get(i + 16..i + 16 + tamanho).unwrap();
            let nome = String::from_utf8_lossy(nome)
                .trim_end_matches('\0')
                .to_owned();
            vistos.push((mascara, nome));
            i += 16 + tamanho;
        }
    }
    // SAFETY: o descritor é deste teste.
    unsafe { libc::close(fd) };
    vistos
}

#[test]
fn o_que_o_ajudante_poe_pela_montagem_aparece_para_quem_observa_a_pasta() {
    let Some(b) = montada("observar") else { return };
    let fd = observar(&b.ponto.join("sub"));
    // Montado no cache, como o ajudante faz, e posto no lugar pela montagem: a pasta de controle
    // existe para o próprio processo.
    std::fs::write(b.conteudo.join(".inputremote/novo"), b"chegou").unwrap();
    std::fs::rename(
        b.ponto.join(".inputremote/novo"),
        b.ponto.join("sub/novo.txt"),
    )
    .unwrap();
    std::fs::create_dir(b.ponto.join("sub/pasta-nova")).unwrap();
    let vistos = eventos(fd, Duration::from_secs(2));
    let viu = |mascara: u32, nome: &str| vistos.iter().any(|(m, n)| m & mascara != 0 && n == nome);
    assert!(viu(libc::IN_MOVED_TO, "novo.txt"), "{vistos:?}");
    assert!(viu(libc::IN_CREATE, "pasta-nova"), "{vistos:?}");
    assert_eq!(
        std::fs::read(b.conteudo.join("sub/novo.txt")).unwrap(),
        b"chegou"
    );
    // Para os outros processos, e na listagem, a pasta de controle continua não existindo.
    let de_fora = std::process::Command::new("stat")
        .arg(b.ponto.join(".inputremote"))
        .output()
        .unwrap();
    assert!(
        !de_fora.status.success(),
        "outro processo não vê a pasta de controle"
    );
    let nomes: Vec<_> = std::fs::read_dir(&b.ponto)
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    assert!(!nomes.iter().any(|n| n == ".inputremote"));
}

#[test]
fn com_programas_esperando_conteudo_o_ajudante_ainda_mexe_na_pasta() {
    let Some(b) = montada("esperando") else {
        return;
    };
    // Mais leitores esperando a rede do que threads na montagem.
    let leitores: Vec<_> = (0..6)
        .map(|i| {
            sem(&b, &format!("e{i}.bin"), 600_000);
            let alvo = b.ponto.join(format!("e{i}.bin"));
            std::thread::spawn(move || std::fs::read(alvo))
        })
        .collect();
    let mut pedidos = Vec::new();
    while pedidos.len() < 6 {
        let Pedido::Buscar(busca) = b.pedidos.recv_timeout(Duration::from_secs(10)).unwrap() else {
            panic!("esperava uma busca")
        };
        pedidos.push(busca);
    }
    // Com os seis esperando, o ajudante cria e move pela montagem — sem ninguém o atender, travava.
    let inicio = std::time::Instant::now();
    std::fs::create_dir(b.ponto.join("pasta-nova")).unwrap();
    std::fs::rename(b.ponto.join("sub"), b.ponto.join("sub-movida")).unwrap();
    assert!(
        inicio.elapsed() < Duration::from_secs(2),
        "{:?}",
        inicio.elapsed()
    );
    // Solta os leitores: nada vem.
    for busca in pedidos {
        falhar(0, busca.transferencia, (busca.offset, busca.tamanho)).unwrap();
    }
    for leitor in leitores {
        while !leitor.is_finished() {
            if let Ok(Pedido::Buscar(busca)) = b.pedidos.recv_timeout(Duration::from_millis(50)) {
                if busca.tamanho == u64::MAX {
                    b.montagem.pronto(&busca.caminho, false);
                } else {
                    falhar(0, busca.transferencia, (busca.offset, busca.tamanho)).unwrap();
                }
            }
        }
        assert!(leitor.join().unwrap().is_err());
    }
}
