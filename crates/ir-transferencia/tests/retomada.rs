//! A cópia que o canal de arquivos não conseguia levar, ou que largava no meio.
//!
//! O relato ([log 55](../../../docs/logs/55-o-manifesto-que-nao-cabia.md)): copiar uma pasta de
//! jogo no Linux e levar o mouse ao Windows terminava em "a cópia não atravessou · a conexão de
//! arquivos caiu". Duas coisas, contra o canal inteiro — TCP, Noise e codec:
//!
//! - o manifesto de milhares de arquivos não cabia num quadro; agora vai em partes;
//! - uma queda de verdade no meio da cópia era o fim dela; agora ela recomeça sozinha quando o
//!   enlace volta, e os dois lados mostram a espera em vez da falha.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod comum;

use std::path::Path;
use std::time::Duration;

use comum::{esperar, limpar, parear, subir, terminou};
use ir_crypto::Identity;
use ir_ipc::transferencia::{Fase, Sentido};
use ir_transferencia::{Destino, Leitor};

/// Quantos arquivos a pasta de jogo do teste tem: o bastante para o manifesto passar, com folga, de
/// um quadro TCP.
const ARQUIVOS: usize = 3_000;

fn escrever(caminho: &Path, conteudo: &[u8]) {
    std::fs::create_dir_all(caminho.parent().unwrap()).unwrap();
    std::fs::write(caminho, conteudo).unwrap();
}

fn contar_arquivos(raiz: &Path) -> usize {
    std::fs::read_dir(raiz)
        .unwrap()
        .map(|entrada| entrada.unwrap().path())
        .map(|caminho| {
            if caminho.is_dir() {
                contar_arquivos(&caminho)
            } else {
                1
            }
        })
        .sum()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn uma_pasta_com_milhares_de_arquivos_atravessa_pelo_canal_de_verdade() {
    let mut a = subir("milhares-a");
    let mut b = subir("milhares-b");
    parear(&a, &b);
    parear(&b, &a);
    let pasta = a.pasta.join("Jogo eletronica 2");
    for n in 0..ARQUIVOS {
        let alvo = pasta.join(format!("Data/Textures/textura-{n:05}.png"));
        escrever(&alvo, format!("conteúdo {n}").as_bytes());
    }

    assert!(a.pedidos.enviar(vec![pasta], Leitor::Proprio));
    let enviado = esperar(&mut a.avisos, Sentido::Enviando, prazo(90), terminou)
        .await
        .expect("a cópia terminou");
    assert!(
        matches!(enviado.fase, Fase::Concluida { .. }),
        "{enviado:?}"
    );
    let recebido = esperar(&mut b.avisos, Sentido::Recebendo, prazo(30), terminou)
        .await
        .expect("chegou do outro lado");
    let Fase::Concluida { destino } = recebido.fase else {
        panic!("{recebido:?}");
    };
    let destino = Path::new(&destino);
    assert_eq!(contar_arquivos(destino), ARQUIVOS);
    assert_eq!(
        std::fs::read(destino.join("Data/Textures/textura-02999.png")).unwrap(),
        b"conte\xc3\xbado 2999".to_vec()
    );

    limpar([a, b]);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_copia_que_o_enlace_largou_no_meio_recomeca_sozinha() {
    let mut a = subir("queda-a");
    let mut b = subir("queda-b");
    parear(&a, &b);
    parear(&b, &a);
    let arquivo = a.pasta.join("grande.bin");
    let conteudo: Vec<u8> = (0..16 * 1024 * 1024)
        .map(|n: usize| u8::try_from(n % 251).unwrap())
        .collect();
    escrever(&arquivo, &conteudo);

    assert!(a.pedidos.enviar(vec![arquivo], Leitor::Proprio));
    esperar(&mut a.avisos, Sentido::Enviando, prazo(30), |t| {
        t.fase == Fase::Andando && t.bytes_feitos > 0
    })
    .await
    .expect("a cópia começou a andar");

    // B passa a esperar outro par: o enlace com A cai no meio da cópia, como numa queda da rede.
    b.pedidos.trocar_destino(Destino {
        chave: Some(Identity::generate().public()),
        alvo: None,
    });
    let espera = esperar(&mut a.avisos, Sentido::Enviando, prazo(30), |t| {
        t.fase == Fase::AguardandoConexao || terminou(t)
    })
    .await
    .expect("quem envia contou o que houve");
    assert_eq!(
        espera.fase,
        Fase::AguardandoConexao,
        "a queda mostra a espera, e não a falha"
    );

    // A rede voltou.
    parear(&b, &a);
    let enviado = esperar(&mut a.avisos, Sentido::Enviando, prazo(90), terminou)
        .await
        .expect("a cópia terminou");
    assert!(
        matches!(enviado.fase, Fase::Concluida { .. }),
        "recomeçou sozinha e chegou: {enviado:?}"
    );
    let recebido = esperar(&mut b.avisos, Sentido::Recebendo, prazo(30), terminou)
        .await
        .expect("chegou do outro lado");
    let Fase::Concluida { destino } = recebido.fase else {
        panic!("quem recebe também não fala em falha: {recebido:?}");
    };
    assert_eq!(std::fs::read(destino).unwrap(), conteudo, "inteira e certa");

    limpar([a, b]);
}

const fn prazo(segundos: u64) -> Duration {
    Duration::from_secs(segundos)
}
