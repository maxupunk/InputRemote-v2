//! O canal de arquivos acompanha o par: sobe sem nenhum, e passa a funcionar quando um é pareado —
//! sem reiniciar o serviço.
//!
//! O defeito: a chave era lida uma vez, na subida. Parear depois deixava arquivos indisponíveis até
//! reiniciar, enquanto teclado e mouse já funcionavam — o tipo de diferença que faz o produto
//! parecer quebrado pela metade.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use ir_crypto::{Identity, PublicKey};
use ir_ipc::Aviso;
use ir_ipc::transferencia::{Fase, Motivo, Sentido, Transferencia};
use ir_transferencia::{
    Ajuste, Cota, Destino, Leitor, Localizador, Pedidos, iniciar, sem_localizador,
};
use ir_transporte::Endereco;
use tokio::sync::broadcast;

/// Uma porta livre agora. Há uma janela até o serviço a usar, pequena demais para importar aqui.
fn porta_livre() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

struct Maquina {
    identidade: Arc<Identity>,
    porta: u16,
    pedidos: Pedidos,
    avisos: broadcast::Receiver<Aviso>,
    pasta: PathBuf,
}

fn subir(nome: &str, localizar: Localizador) -> Maquina {
    let pasta = std::env::temp_dir().join(format!("ir-destino-{nome}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&pasta);
    std::fs::create_dir_all(pasta.join("recebidos")).unwrap();
    let identidade = Arc::new(Identity::generate());
    let porta = porta_livre();
    let (avisos, ouvinte) = broadcast::channel(64);
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

fn apontar(de: &Maquina, para: &Maquina) {
    let alvo = SocketAddr::from(([127, 0, 0, 1], para.porta));
    de.pedidos.trocar_destino(Destino {
        chave: Some(para.identidade.public()),
        alvo: Some(Endereco::Rede(alvo)),
    });
}

/// O próximo aviso de transferência que termina, neste sentido.
async fn fim(avisos: &mut broadcast::Receiver<Aviso>, sentido: Sentido) -> Transferencia {
    let espera = async {
        loop {
            if let Ok(Aviso::Transferencia(t)) = avisos.recv().await
                && t.sentido == sentido
                && matches!(t.fase, Fase::Concluida { .. } | Fase::Parada(_))
            {
                return t;
            }
        }
    };
    tokio::time::timeout(Duration::from_secs(20), espera)
        .await
        .expect("a transferência não terminou a tempo")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn sem_par_recusa_com_motivo_e_com_par_novo_envia_sem_reiniciar() {
    let mut a = subir("a", sem_localizador());
    let mut b = subir("b", sem_localizador());
    let arquivo = a.pasta.join("bilhete.txt");
    std::fs::write(&arquivo, b"chegou sem reiniciar").unwrap();

    assert!(a.pedidos.enviar(vec![arquivo.clone()], Leitor::Proprio));
    let recusa = fim(&mut a.avisos, Sentido::Enviando).await;
    assert!(
        matches!(&recusa.fase, Fase::Parada(Motivo::Outro(m)) if m.contains("par")),
        "{recusa:?}"
    );

    // Pareiam agora. Nenhum dos dois reinicia.
    apontar(&a, &b);
    apontar(&b, &a);
    assert!(a.pedidos.enviar(vec![arquivo], Leitor::Proprio));
    let enviado = fim(&mut a.avisos, Sentido::Enviando).await;
    assert!(
        matches!(enviado.fase, Fase::Concluida { .. }),
        "{enviado:?}"
    );
    let recebido = fim(&mut b.avisos, Sentido::Recebendo).await;
    let Fase::Concluida { destino } = recebido.fase else {
        panic!("{recebido:?}");
    };
    assert_eq!(
        std::fs::read(destino).unwrap(),
        b"chegou sem reiniciar".to_vec()
    );

    for pasta in [a.pasta, b.pasta] {
        let _ = std::fs::remove_dir_all(pasta);
    }
}

/// Uma rede local de mentira: quem responde "estou aqui" à descoberta.
type Rede = Arc<Mutex<Vec<(PublicKey, SocketAddr)>>>;

fn localizador_da(rede: &Rede) -> Localizador {
    let rede = Arc::clone(rede);
    Arc::new(move |chave| {
        let achado = rede
            .lock()
            .unwrap()
            .iter()
            .find(|(quem, _)| *quem == chave)
            .map(|(_, onde)| *onde);
        Box::pin(async move { achado })
    })
}

/// O defeito da bancada: pareados pelo Bluetooth, cada lado só sabia o endereço do rádio do outro. O
/// canal de arquivos, que só disca endereço de rede, esperava para sempre — o texto atravessava, e
/// copiar um arquivo não fazia nada. Agora ele pergunta à rede onde o par está.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn pareados_pelo_bluetooth_os_arquivos_acham_o_par_na_rede() {
    let rede: Rede = Arc::default();
    let mut a = subir("radio-a", localizador_da(&rede));
    let mut b = subir("radio-b", localizador_da(&rede));
    for maquina in [&a, &b] {
        let onde = SocketAddr::from(([127, 0, 0, 1], maquina.porta));
        rede.lock()
            .unwrap()
            .push((maquina.identidade.public(), onde));
    }
    for (de, para, radio) in [(&a, &b, "AC:50:DE:47:EB:28"), (&b, &a, "74:13:EA:A6:5A:99")] {
        de.pedidos.trocar_destino(Destino {
            chave: Some(para.identidade.public()),
            alvo: Endereco::ler(radio),
        });
    }

    let arquivo = a.pasta.join("pelo-radio.txt");
    std::fs::write(&arquivo, b"achado na rede").unwrap();
    assert!(a.pedidos.enviar(vec![arquivo], Leitor::Proprio));
    let enviado = fim(&mut a.avisos, Sentido::Enviando).await;
    assert!(
        matches!(enviado.fase, Fase::Concluida { .. }),
        "{enviado:?}"
    );
    let recebido = fim(&mut b.avisos, Sentido::Recebendo).await;
    let Fase::Concluida { destino } = recebido.fase else {
        panic!("{recebido:?}");
    };
    assert_eq!(std::fs::read(destino).unwrap(), b"achado na rede".to_vec());

    for pasta in [a.pasta, b.pasta] {
        let _ = std::fs::remove_dir_all(pasta);
    }
}
