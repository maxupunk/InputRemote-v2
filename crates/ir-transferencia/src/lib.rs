//! A transferência de arquivos em curso: o canal de dados conduzido.
//!
//! Este módulo **não** faz parte do ator. É de propósito: o ator gira no compasso de 5 ms da
//! entrada, e um bloco de 60 KiB passando por ali é exatamente o defeito que o critério de saída
//! da Etapa 8 proíbe — *"transferência de 5 GB sem degradar a latência da entrada além de 10%"*.
//!
//! O ator só conhece uma coisa daqui: por onde pedir um envio ([`Pedidos`]). O resto acontece
//! sozinho, e falhar aqui não toca a sessão de entrada
//! ([01, §3.3](../../../docs/01-visao-e-escopo.md)).
//!
//! Ver [ADR-0010](../../../docs/adr/0010-canal-de-dados-em-tcp-proprio.md).
//!
//! # Por que um crate, e não um módulo do serviço
//!
//! Porque ele é o único lugar que conhece o motor (`ir-files`) e a porta (`ir-transporte`) ao mesmo
//! tempo — a mesma fronteira que o `ir-transporte` desenha para os portadores de entrada. E porque
//! o `ir-daemon` estourou o orçamento de 2 500 linhas de produção quando isto entrou nele, que é
//! exatamente o sinal que aquele limite existe para dar: falta uma fronteira, não um número maior
//! ([09, §1](../../../docs/09-padroes-de-codigo.md)).

#![forbid(unsafe_code)]

mod enviando;
mod recebendo;
mod sessao;

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use ir_crypto::{Identity, PublicKey};
/// A cota de quem recebe, reexportada para quem monta o [`Ajuste`] não precisar de `ir-files`.
///
/// Quem configura a transferência não tem por que conhecer o motor dela
/// ([02, §2](../../../docs/02-arquitetura.md)).
pub use ir_files::Cota;
use ir_ipc::Aviso;
use ir_ipc::transferencia::{Fase, Motivo, Sentido, Transferencia};
use ir_transporte::dados::{EnlaceDeDados, Porta, ficar_com_o_proprio};
use tokio::sync::{broadcast, mpsc};
use tracing::{debug, info, warn};

/// Quanto esperar antes de tentar de novo quando o par não atende.
///
/// O mesmo espaçamento da reconexão de entrada. Discar mais rápido não faria a outra máquina subir
/// antes, e encheria o registro — foi a lição do relançamento do agente ([log 29](../../../docs/logs/29-o-agente-que-nunca-dizia-por-que.md)).
const ESPERA_ENTRE_TENTATIVAS: Duration = Duration::from_secs(3);

/// Quanto o lado não preferido espera antes de discar também.
///
/// A regra de colisão diz quem disca primeiro. Mas ela sozinha travaria o caso em que **só** o lado
/// não preferido conhece o endereço do outro — ele esperaria para sempre por uma conexão que
/// ninguém vai abrir. Depois desta carência ele disca também.
const CARENCIA_DO_NAO_PREFERIDO: Duration = Duration::from_secs(5);

/// Por onde o ator pede um envio.
#[derive(Debug, Clone)]
pub struct Pedidos {
    fila: mpsc::UnboundedSender<Vec<PathBuf>>,
}

impl Pedidos {
    /// Uma alça que não leva a lugar nenhum.
    ///
    /// Para bancada de teste do serviço, que exercita o ator sem canal de arquivos. Não finge
    /// sucesso: [`Self::enviar`] devolve `false`, e o serviço responde ao pedido com falha — que é
    /// a verdade naquele cenário.
    #[must_use]
    pub fn desligada() -> Self {
        let (fila, _) = mpsc::unbounded_channel();
        Self { fila }
    }

    /// Pede o envio destes caminhos. `false` quando a transferência não está de pé.
    #[must_use]
    pub fn enviar(&self, caminhos: Vec<PathBuf>) -> bool {
        self.fila.send(caminhos).is_ok()
    }
}

/// O que a transferência precisa para existir.
#[derive(Debug)]
pub struct Ajuste {
    /// A porta TCP. A mesma número do UDP ([03, §10](../../../docs/03-protocolo.md)).
    pub porta: u16,
    /// Onde os arquivos recebidos ficam.
    pub recebidos: PathBuf,
    /// Quanto esta máquina aceita receber.
    pub cota: Cota,
    /// A identidade desta máquina.
    pub identidade: Arc<Identity>,
    /// A chave fixada do par, quando já há um.
    pub par: Option<PublicKey>,
    /// Onde alcançar o par pela rede, quando se sabe.
    pub alvo: Option<SocketAddr>,
    /// Para contar à interface o que está acontecendo.
    pub avisos: broadcast::Sender<Aviso>,
}

/// Sobe a tarefa de transferência e devolve por onde pedir envios.
#[must_use]
pub fn iniciar(ajuste: Ajuste) -> Pedidos {
    let (fila, pedidos) = mpsc::unbounded_channel();
    tokio::spawn(servir(ajuste, pedidos));
    Pedidos { fila }
}

/// O laço de vida do canal de dados: tem enlace, usa; não tem, consegue um.
async fn servir(ajuste: Ajuste, mut pedidos: mpsc::UnboundedReceiver<Vec<PathBuf>>) {
    let Some(par) = ajuste.par else {
        // Sem par fixado não há com quem trocar arquivo, e abrir a porta seria convidar
        // conexão que nenhuma identidade autorizaria.
        info!("sem par pareado; arquivos indisponíveis até haver um");
        recusar_tudo(
            &ajuste,
            &mut pedidos,
            Motivo::Outro("não há par pareado".to_owned()),
        )
        .await;
        return;
    };
    let porta = match Porta::abrir(ajuste.porta, Arc::clone(&ajuste.identidade)).await {
        Ok(porta) => porta,
        Err(erro) => {
            warn!(%erro, "não consegui abrir o TCP de arquivos");
            recusar_tudo(&ajuste, &mut pedidos, Motivo::Outro(erro.to_string())).await;
            return;
        }
    };
    info!(porta = ajuste.porta, "canal de arquivos no ar");

    loop {
        let enlace = obter(&porta, &ajuste, par).await;
        info!("canal de arquivos estabelecido");
        sessao::conduzir(enlace, &ajuste, &mut pedidos).await;
        warn!("o canal de arquivos caiu; teclado e mouse não foram afetados");
        let _ = ajuste.avisos.send(Aviso::Transferencia(Transferencia {
            sentido: Sentido::Recebendo,
            nome: String::new(),
            bytes_feitos: 0,
            bytes_total: 0,
            fase: Fase::Parada(Motivo::CanalCaiu),
        }));
    }
}

/// Consegue um enlace: atende quem chega, e disca quando é a vez deste lado.
///
/// Os dois lados escutam e os dois podem ter o endereço do outro. Quem disca primeiro é decidido
/// pela regra da chave maior, sem trocar mensagem; o outro só disca depois da carência, para o caso
/// de ser ele o único que sabe o endereço.
async fn obter(porta: &Porta, ajuste: &Ajuste, par: PublicKey) -> EnlaceDeDados {
    let nossa = ajuste.identidade.public();
    let carencia = if ficar_com_o_proprio(nossa, par) {
        Duration::ZERO
    } else {
        CARENCIA_DO_NAO_PREFERIDO
    };

    loop {
        tokio::select! {
            atendido = porta.aceitar(par) => match atendido {
                Ok(enlace) => return enlace,
                Err(erro) => debug!(%erro, "conexão de arquivos recusada na porta"),
            },
            enlace = discar_depois(porta, ajuste.alvo, par, carencia) => {
                if let Some(enlace) = enlace {
                    return enlace;
                }
            }
        }
    }
}

/// Disca depois da carência, e devolve `None` quando não deu — para o laço tentar de novo.
///
/// Sem `alvo` este futuro **nunca** resolve, o que deixa o `select!` só com o lado que atende. É o
/// caso da máquina que não sabe o endereço da outra.
async fn discar_depois(
    porta: &Porta,
    alvo: Option<SocketAddr>,
    par: PublicKey,
    carencia: Duration,
) -> Option<EnlaceDeDados> {
    let alvo = alvo?;
    tokio::time::sleep(carencia).await;
    match porta.discar(alvo, par).await {
        Ok(enlace) => Some(enlace),
        Err(erro) => {
            debug!(%erro, %alvo, "o par ainda não atende no canal de arquivos");
            tokio::time::sleep(ESPERA_ENTRE_TENTATIVAS).await;
            None
        }
    }
}

/// Responde a todo pedido com a mesma recusa, enquanto o serviço viver.
///
/// Dizer não é melhor que ficar calado: um pedido que some deixa o usuário achando que a cópia foi
/// feita.
async fn recusar_tudo(
    ajuste: &Ajuste,
    pedidos: &mut mpsc::UnboundedReceiver<Vec<PathBuf>>,
    motivo: Motivo,
) {
    while let Some(caminhos) = pedidos.recv().await {
        let nome = caminhos
            .first()
            .and_then(|caminho| caminho.file_name())
            .map_or_else(String::new, |nome| nome.to_string_lossy().into_owned());
        let _ = ajuste.avisos.send(Aviso::Transferencia(Transferencia {
            sentido: Sentido::Enviando,
            nome,
            bytes_feitos: 0,
            bytes_total: 0,
            fase: Fase::Parada(motivo.clone()),
        }));
    }
}
