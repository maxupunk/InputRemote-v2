//! O que os testes de travessia compartilham: pasta temporária, árvores de exemplo, e o laço que
//! conduz uma transferência inteira de um lado ao outro.
//!
//! O condutor é o ponto: ele é o que o `ir-daemon` vai fazer, escrito uma vez aqui, sem socket
//! nenhum no meio. Se a transferência funciona contra este condutor, o que falta provar na bancada
//! é o transporte — e o transporte tem os próprios testes.

#![allow(
    dead_code,
    unreachable_pub,
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic
)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use ir_files::error::FileError;
use ir_files::{Abertura, Cota, Envio, Reacao, Recepcao, manifesto};
use ir_proto::message::{BulkMessage, RejectReason, TransferId};

static CONTADOR: AtomicU32 = AtomicU32::new(0);

/// Uma pasta que se apaga quando sai de escopo.
#[derive(Debug)]
pub struct Temp {
    caminho: PathBuf,
}

impl Temp {
    pub fn caminho(&self) -> &Path {
        &self.caminho
    }

    /// Uma subpasta desta, já criada.
    pub fn sub(&self, nome: &str) -> PathBuf {
        let caminho = self.caminho.join(nome);
        std::fs::create_dir_all(&caminho).expect("criar subpasta");
        caminho
    }
}

impl Drop for Temp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.caminho);
    }
}

pub fn temp(rotulo: &str) -> Temp {
    let n = CONTADOR.fetch_add(1, Ordering::Relaxed);
    let caminho =
        std::env::temp_dir().join(format!("ir-files-t-{rotulo}-{}-{n}", std::process::id()));
    let _ = std::fs::remove_dir_all(&caminho);
    std::fs::create_dir_all(&caminho).expect("criar a pasta temporária");
    Temp { caminho }
}

/// Escreve um arquivo, criando os pais.
pub async fn escrever(caminho: &Path, conteudo: &[u8]) {
    if let Some(pai) = caminho.parent() {
        tokio::fs::create_dir_all(pai).await.unwrap();
    }
    tokio::fs::write(caminho, conteudo).await.unwrap();
}

/// Uma árvore com os casos que costumam quebrar: vazio, acento, espaço, maior que um bloco, e um
/// tamanho que não é múltiplo do bloco.
pub async fn arvore_de_exemplo(dentro: &Path) -> PathBuf {
    let raiz = dentro.join("relatório de janeiro");
    escrever(&raiz.join("resumo.txt"), b"tres linhas\ne um fim\n").await;
    escrever(&raiz.join("vazio.dat"), b"").await;
    escrever(
        &raiz.join("anexos").join("planilha com espaço.bin"),
        &conteudo_variado(ir_proto::limits::MAX_FILE_BLOCK * 2 + 1234),
    )
    .await;
    escrever(
        &raiz.join("anexos").join("fundo").join("nota.md"),
        "acentuação e emoji 🙂".as_bytes(),
    )
    .await;
    tokio::fs::create_dir_all(raiz.join("pasta vazia"))
        .await
        .unwrap();
    raiz
}

/// Bytes que não são todos iguais: conteúdo constante esconde bloco trocado de lugar.
pub fn conteudo_variado(tamanho: usize) -> Vec<u8> {
    (0..tamanho)
        .map(|n| u8::try_from((n * 7 + n / 251) % 251).unwrap_or(0))
        .collect()
}

/// Como uma travessia terminou.
#[derive(Debug)]
pub enum Fim {
    /// Publicado aqui.
    Publicado(PathBuf),
    /// O destino recusou antes de começar.
    Recusado(RejectReason),
    /// Falhou no meio.
    Falhou(FileError),
}

impl Fim {
    pub fn publicado(self) -> PathBuf {
        match self {
            Self::Publicado(caminho) => caminho,
            outro => panic!("esperava publicação, veio {outro:?}"),
        }
    }

    pub fn falha(self) -> FileError {
        match self {
            Self::Falhou(erro) => erro,
            outro => panic!("esperava falha, veio {outro:?}"),
        }
    }

    pub fn recusa(self) -> RejectReason {
        match self {
            Self::Recusado(motivo) => motivo,
            outro => panic!("esperava recusa, veio {outro:?}"),
        }
    }
}

/// Conduz uma transferência inteira, sem mexer em nada.
pub async fn atravessar(origem: &[PathBuf], recebidos: &Path, cota: Cota) -> Fim {
    atravessar_com(origem, recebidos, cota, |_, m| Some(m)).await
}

/// Conduz uma transferência, deixando o teste interferir em cada mensagem.
///
/// A função recebe o número de ordem da mensagem e a mensagem, e devolve o que deve chegar ao
/// destino — ou `None` para descartá-la. É assim que os testes de adulteração e de bloco grande
/// demais são escritos sem precisar de um par hostil de verdade.
pub async fn atravessar_com<F>(
    origem: &[PathBuf],
    recebidos: &Path,
    cota: Cota,
    mut interferir: F,
) -> Fim
where
    F: FnMut(usize, BulkMessage) -> Option<BulkMessage>,
{
    let plano = match manifesto::montar(TransferId(1), origem).await {
        Ok(plano) => plano,
        Err(erro) => return Fim::Falhou(erro),
    };
    let mut envio = Envio::novo(plano);
    let (id, itens, total) = match envio.manifesto() {
        BulkMessage::Manifest {
            id,
            items,
            total_bytes,
        } => (id, items, total_bytes),
        outro => panic!("o manifesto não é um manifesto: {outro:?}"),
    };

    let mut recepcao = match Recepcao::abrir(recebidos, (id, itens, total), cota, None).await {
        Ok(Abertura::Aceita { recepcao, .. }) => recepcao,
        Ok(Abertura::Recusada { motivo, .. }) => return Fim::Recusado(motivo),
        Err(erro) => return Fim::Falhou(erro),
    };

    let mut ordem = 0usize;
    loop {
        let mensagem = match envio.proxima().await {
            Ok(Some(mensagem)) => mensagem,
            Ok(None) => break,
            Err(erro) => return Fim::Falhou(erro),
        };
        ordem += 1;
        let Some(mensagem) = interferir(ordem, mensagem) else {
            continue;
        };
        match recepcao.aplicar(mensagem).await {
            Ok(Reacao::Cancelada(motivo)) => {
                return Fim::Falhou(FileError::Violacao(match motivo {
                    ir_proto::message::CancelReason::UserRequested => "o par cancelou",
                    _ => "o par interrompeu",
                }));
            }
            Ok(_) => {}
            Err(erro) => return Fim::Falhou(erro),
        }
    }
    let (saiu, entrou) = (envio.enviados(), recepcao.escritos());
    match recepcao.concluir().await {
        Ok(caminho) => {
            // Conferido só no caminho de sucesso: quando o teste interferiu de propósito, os dois
            // números *devem* divergir, e é isso que o teste está medindo.
            assert_eq!(
                saiu, entrou,
                "a travessia completou mas o que saiu e o que entrou não batem"
            );
            Fim::Publicado(caminho)
        }
        Err(erro) => Fim::Falhou(erro),
    }
}

/// Todo o conteúdo de uma árvore, por caminho relativo. Pastas entram com valor vazio.
pub fn ler_arvore(raiz: &Path) -> BTreeMap<String, Vec<u8>> {
    let mut tudo = BTreeMap::new();
    let mut pilha = vec![raiz.to_path_buf()];
    while let Some(pasta) = pilha.pop() {
        let entradas = std::fs::read_dir(&pasta).expect("ler a pasta");
        for entrada in entradas {
            let caminho = entrada.expect("entrada").path();
            let relativo = caminho
                .strip_prefix(raiz)
                .expect("dentro da raiz")
                .to_string_lossy()
                .replace('\\', "/");
            if caminho.is_dir() {
                tudo.insert(relativo, Vec::new());
                pilha.push(caminho);
            } else {
                tudo.insert(relativo, std::fs::read(&caminho).expect("ler o arquivo"));
            }
        }
    }
    tudo
}

/// Se sobrou alguma montagem parcial na pasta de recebidos.
pub fn tem_montagem_parcial(recebidos: &Path) -> bool {
    std::fs::read_dir(recebidos).is_ok_and(|entradas| {
        entradas
            .flatten()
            .any(|e| e.file_name().to_string_lossy().starts_with(".parcial-"))
    })
}
