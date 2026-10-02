//! O que os testes do canal de arquivos têm em comum: máquinas de verdade, cada uma com a sua porta,
//! a sua identidade e a sua pasta de recebidos, falando por TCP em `127.0.0.1`.
//!
//! Cada arquivo de teste é um crate próprio e usa uma parte disto; o resto ficaria como código
//! morto em cada um.
#![allow(dead_code, unreachable_pub)]

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use ir_crypto::Identity;
use ir_ipc::Aviso;
use ir_ipc::transferencia::{Fase, Sentido, Transferencia};
use ir_transferencia::{Ajuste, Cota, Destino, Localizador, Pedidos, iniciar, sem_localizador};
use ir_transporte::Endereco;
use tokio::sync::broadcast;

/// Uma máquina com o canal de arquivos no ar.
pub struct Maquina {
    pub identidade: Arc<Identity>,
    pub porta: u16,
    pub pedidos: Pedidos,
    pub avisos: broadcast::Receiver<Aviso>,
    pub pasta: PathBuf,
}

/// Uma porta livre agora. Há uma janela até o serviço a usar, pequena demais para importar aqui.
pub fn porta_livre() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

/// Uma máquina numa porta livre, sem descoberta na rede.
pub fn subir(rotulo: &str) -> Maquina {
    subir_com(rotulo, porta_livre(), sem_localizador())
}

/// Uma máquina nesta porta, achando o par por `localizar`.
pub fn subir_com(rotulo: &str, porta: u16, localizar: Localizador) -> Maquina {
    let pasta = std::env::temp_dir().join(format!("ir-{rotulo}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&pasta);
    std::fs::create_dir_all(pasta.join("recebidos")).unwrap();
    let identidade = Arc::new(Identity::generate());
    // Folgado: o andamento sai cinco vezes por segundo, e um ouvinte atrasado perderia o fim.
    let (avisos, ouvinte) = broadcast::channel(1024);
    let pedidos = iniciar(Ajuste {
        porta,
        recebidos: pasta.join("recebidos"),
        cota: Cota::default(),
        identidade: Arc::clone(&identidade),
        destino: Destino::default(),
        localizar,
        avisos,
    });
    Maquina {
        identidade,
        porta,
        pedidos,
        avisos: ouvinte,
        pasta,
    }
}

/// Aponta `de` para `para`: a chave dele, e onde ele escuta.
pub fn parear(de: &Maquina, para: &Maquina) {
    let alvo = SocketAddr::from(([127, 0, 0, 1], para.porta));
    de.pedidos.trocar_destino(Destino {
        chave: Some(para.identidade.public()),
        alvo: Some(Endereco::Rede(alvo)),
    });
}

/// O primeiro aviso deste sentido que satisfaz `condicao`, dentro do prazo.
pub async fn esperar(
    avisos: &mut broadcast::Receiver<Aviso>,
    sentido: Sentido,
    prazo: Duration,
    condicao: impl Fn(&Transferencia) -> bool,
) -> Option<Transferencia> {
    let procurar = async {
        loop {
            if let Ok(Aviso::Transferencia(t)) = avisos.recv().await
                && t.sentido == sentido
                && condicao(&t)
            {
                return t;
            }
        }
    };
    tokio::time::timeout(prazo, procurar).await.ok()
}

/// Se a cópia terminou, bem ou mal.
pub fn terminou(t: &Transferencia) -> bool {
    matches!(t.fase, Fase::Concluida { .. } | Fase::Parada(_))
}

/// Apaga as pastas das máquinas do teste.
pub fn limpar(maquinas: impl IntoIterator<Item = Maquina>) {
    for maquina in maquinas {
        let _ = std::fs::remove_dir_all(maquina.pasta);
    }
}
