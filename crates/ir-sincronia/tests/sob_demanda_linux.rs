//! A réplica sob demanda do Linux de ponta a ponta: dois ajudantes de verdade, e a pasta recebida
//! montada por FUSE. Abrir um arquivo que não veio pede o conteúdo à origem e espera; sem o outro
//! computador, abrir falha com "rede inalcançável"; o que se grava pela montagem chega à origem.
//!
//! Pula sozinho onde não há FUSE (`/dev/fuse` e o `fusermount3`). Na bancada roda num Fedora com
//! `--device /dev/fuse --cap-add SYS_ADMIN`.

#![cfg(target_os = "linux")]
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

use ir_ipc::pastas::ComandoDePasta;
use ir_proto::message::FolderMessage;
use ir_sincronia::{Lugar, Pastas};

struct Lado {
    pastas: Pastas,
    fila: Vec<FolderMessage>,
}

struct Bancada {
    base: PathBuf,
    a: Lado,
    b: Lado,
    pedidos: Receiver<ir_nuvem::Pedido>,
    ligado: bool,
}

impl Drop for Bancada {
    fn drop(&mut self) {
        let ponto = self.base.join("notebook/InputRemote/Projetos");
        let _ = std::process::Command::new("fusermount3")
            .arg("-u")
            .arg(&ponto)
            .output();
        let _ = std::fs::remove_dir_all(&self.base);
    }
}

fn lugar(base: &Path, nome: &str) -> Lugar {
    let lugar = Lugar {
        estado: base.join(nome).join("estado"),
        raiz_padrao: base.join(nome).join("InputRemote"),
        maquina: nome.to_uppercase(),
        fuso_s: 0,
    };
    std::fs::create_dir_all(&lugar.raiz_padrao).unwrap();
    lugar
}

impl Bancada {
    fn nova() -> Self {
        let base = std::env::temp_dir().join(format!("ir-sob-demanda-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let (envia, pedidos) = mpsc::channel();
        let mut b = Pastas::abrir(lugar(&base, "notebook"));
        b.usar_nuvem(Arc::new(move |pedido| {
            let _ = envia.send(pedido);
        }));
        let mut bancada = Self {
            a: Lado {
                pastas: Pastas::abrir(lugar(&base, "desktop")),
                fila: Vec::new(),
            },
            b: Lado {
                pastas: b,
                fila: Vec::new(),
            },
            base,
            pedidos,
            ligado: false,
        };
        bancada.enlace(true);
        bancada
    }

    fn enlace(&mut self, de_pe: bool) {
        self.ligado = de_pe;
        let (a, b) = (&mut self.a, &mut self.b);
        a.pastas.enlace(de_pe, true, "NOTEBOOK".into(), &mut a.fila);
        b.pastas.enlace(de_pe, true, "DESKTOP".into(), &mut b.fila);
        self.rodar();
    }

    /// Uma volta do laço dos dois ajudantes: os pedidos da montagem, o disco, as mensagens.
    fn volta(&mut self) -> bool {
        while let Ok(pedido) = self.pedidos.try_recv() {
            self.b.pastas.da_nuvem(pedido, &mut self.b.fila);
        }
        for lado in [&mut self.a, &mut self.b] {
            lado.pastas.varrer(None, &mut lado.fila);
            lado.pastas.andar(&mut lado.fila);
        }
        let de_a = std::mem::take(&mut self.a.fila);
        let de_b = std::mem::take(&mut self.b.fila);
        if !self.ligado {
            return false;
        }
        let houve = !de_a.is_empty() || !de_b.is_empty();
        for mensagem in de_a {
            self.b.pastas.do_par(mensagem, &mut self.b.fila);
        }
        for mensagem in de_b {
            self.a.pastas.do_par(mensagem, &mut self.a.fila);
        }
        houve
    }

    fn rodar(&mut self) {
        for _ in 0..10_000 {
            if !self.volta() {
                return;
            }
        }
        panic!("a conversa não parou");
    }

    /// Roda o laço enquanto alguém, noutra thread, abre um arquivo pela montagem.
    fn enquanto<T: Send + 'static>(&mut self, abrir: impl FnOnce() -> T + Send + 'static) -> T {
        let leitor = std::thread::spawn(abrir);
        let prazo = Instant::now() + Duration::from_secs(20);
        while !leitor.is_finished() {
            assert!(Instant::now() < prazo, "quem abriu ficou esperando");
            self.volta();
            std::thread::sleep(Duration::from_millis(5));
        }
        leitor.join().unwrap()
    }

    /// O desktop compartilha a pasta; o notebook aceita.
    fn compartilhar_e_aceitar(&mut self, origem: &Path) {
        let pedido = ComandoDePasta::Compartilhar {
            caminho: origem.to_string_lossy().into_owned(),
        };
        self.a.pastas.comando(pedido, &mut self.a.fila).unwrap();
        self.rodar();
        let oferta = self.b.pastas.resumo()[0].id;
        let aceitar = ComandoDePasta::Aceitar(oferta);
        self.b.pastas.comando(aceitar, &mut self.b.fila).unwrap();
        self.rodar();
    }
}

#[test]
fn a_pasta_recebida_no_linux_traz_cada_arquivo_so_quando_e_aberto() {
    if !ir_nuvem::suportado(Path::new("/")) {
        eprintln!("sem FUSE nesta máquina; teste pulado");
        return;
    }
    let mut bancada = Bancada::nova();
    let origem = bancada.base.join("desktop/Documentos/Projetos");
    let grande: Vec<u8> = (0..3_000_000u32).map(|i| (i % 251) as u8).collect();
    std::fs::create_dir_all(origem.join("sub")).unwrap();
    std::fs::write(origem.join("notas.txt"), b"escrito no desktop").unwrap();
    std::fs::write(origem.join("sub/grande.bin"), &grande).unwrap();
    bancada.compartilhar_e_aceitar(&origem);

    // A árvore inteira aparece, com os tamanhos certos, antes de qualquer conteúdo vir.
    let visivel = bancada.base.join("notebook/InputRemote/Projetos");
    let grande_aqui = visivel.join("sub/grande.bin");
    assert_eq!(std::fs::metadata(&grande_aqui).unwrap().len(), 3_000_000);
    assert!(
        bancada.b.pastas.resumo()[0]
            .caminho_local
            .ends_with("Projetos"),
        "a pasta mostrada é a montagem, não o cache"
    );

    // Abrir traz o conteúdo da origem, por vários trechos.
    let lido = bancada.enquanto(move || std::fs::read(grande_aqui));
    assert_eq!(lido.unwrap(), grande, "o conteúdo veio inteiro e igual");

    // Sem o outro computador, abrir o que não veio falha na hora, sem travar.
    bancada.enlace(false);
    let notas = visivel.join("notas.txt");
    let erro = bancada.enquanto(move || std::fs::read(notas)).unwrap_err();
    assert_eq!(erro.raw_os_error(), Some(libc_enetunreach()), "{erro}");

    // Gravado pela montagem, offline; quando o enlace volta, chega à origem.
    std::fs::write(visivel.join("do-notebook.txt"), b"feito offline").unwrap();
    bancada.enlace(true);
    bancada.rodar();
    assert_eq!(
        std::fs::read(origem.join("do-notebook.txt")).unwrap(),
        b"feito offline"
    );
    let notas = visivel.join("notas.txt");
    let lido = bancada.enquanto(move || std::fs::read(notas));
    assert_eq!(
        lido.unwrap(),
        b"escrito no desktop",
        "de volta, abre de novo"
    );
}

/// `ENETUNREACH` no Linux, sem trazer o `libc` só para uma constante.
const fn libc_enetunreach() -> i32 {
    101
}
