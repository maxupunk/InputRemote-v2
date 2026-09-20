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
mod localizar;
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

/// Com a autoridade de quem os arquivos são lidos, reexportada pelo mesmo motivo de [`Cota`].
pub use ir_files::{Autorizacao, Leitor};
pub use localizar::{Localizador, sem_localizador};

/// Um pedido de envio: o que mandar, e com a autoridade de quem.
pub(crate) type PedidoDeEnvio = (Vec<PathBuf>, Leitor);
use ir_ipc::Aviso;
use ir_ipc::transferencia::{Fase, Motivo, Sentido, Transferencia};
use ir_transporte::dados::{EnlaceDeDados, Porta, ficar_com_o_proprio};
use tokio::sync::{broadcast, mpsc, watch};
use tracing::{debug, info, warn};

/// Quanto esperar antes de tentar de novo quando o par não atende.
///
/// O mesmo espaçamento da reconexão de entrada. Discar mais rápido não faria a outra máquina subir
/// antes, e encheria o registro — foi a lição do relançamento do agente ([log 29](../../../docs/logs/29-o-agente-que-nunca-dizia-por-que.md)).
const ESPERA_ENTRE_TENTATIVAS: Duration = Duration::from_secs(3);

/// Quanto esperar para perguntar à rede de novo, quando ninguém disse onde o par está.
const ESPERA_SEM_ENDERECO: Duration = Duration::from_secs(15);

/// Quanto o lado não preferido espera antes de discar também.
///
/// A regra de colisão diz quem disca primeiro. Mas ela sozinha travaria o caso em que **só** o lado
/// não preferido conhece o endereço do outro — ele esperaria para sempre por uma conexão que
/// ninguém vai abrir. Depois desta carência ele disca também.
const CARENCIA_DO_NAO_PREFERIDO: Duration = Duration::from_secs(5);

/// Com quem os arquivos são trocados, e onde ele está.
///
/// Muda ao parear e ao esquecer — **sem reiniciar o serviço**. Antes a chave era lida uma vez, na
/// subida, e quem pareava ficava sem arquivos até reiniciar; a entrada, pelo contrário, já valia na
/// hora. O canal agora recomeça sozinho com o par novo.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Destino {
    /// A chave fixada do par, quando há um.
    pub chave: Option<PublicKey>,
    /// Onde alcançá-lo, quando se sabe. Só o de rede é discado: arquivo nunca vai pelo rádio. Sem
    /// ele, o [`Localizador`] acha o par na rede local.
    pub alvo: Option<ir_transporte::Endereco>,
}

/// Por onde o ator pede um envio.
#[derive(Debug, Clone)]
pub struct Pedidos {
    fila: mpsc::UnboundedSender<PedidoDeEnvio>,
    destino: Arc<watch::Sender<Destino>>,
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
        let (destino, _) = watch::channel(Destino::default());
        Self {
            fila,
            destino: Arc::new(destino),
        }
    }

    /// O par mudou: pareou-se um, esqueceu-se o que havia, ou ele foi achado em outro endereço.
    ///
    /// O canal em curso, se era com outro par, cai; o novo sobe sozinho.
    pub fn trocar_destino(&self, destino: Destino) {
        self.destino.send_if_modified(|atual| {
            let mudou = *atual != destino;
            *atual = destino;
            mudou
        });
    }

    /// Pede o envio destes caminhos, lidos com a autoridade de `leitor`. `false` quando a
    /// transferência não está de pé.
    ///
    /// `leitor` não é detalhe: o serviço tem mais autoridade que quem pede, e sem ele o serviço leria
    /// **por** quem pediu o que essa pessoa não leria sozinha (`ir_files::permissao`).
    ///
    /// Caminho vazio é descartado, e um pedido que fica sem nenhum é recusado aqui mesmo: é o único
    /// erro que se vê sem tocar o disco.
    #[must_use]
    pub fn enviar(&self, caminhos: Vec<PathBuf>, leitor: Leitor) -> bool {
        let caminhos: Vec<PathBuf> = caminhos
            .into_iter()
            .filter(|caminho| !caminho.as_os_str().is_empty())
            .collect();
        !caminhos.is_empty() && self.fila.send((caminhos, leitor)).is_ok()
    }
}

/// O que a transferência precisa para existir.
pub struct Ajuste {
    /// A porta TCP. A mesma número do UDP ([03, §10](../../../docs/03-protocolo.md)).
    pub porta: u16,
    /// Onde os arquivos recebidos ficam.
    pub recebidos: PathBuf,
    /// Quanto esta máquina aceita receber.
    pub cota: Cota,
    /// A identidade desta máquina.
    pub identidade: Arc<Identity>,
    /// O par da subida. Depois, quem muda é [`Pedidos::trocar_destino`].
    ///
    /// Qualquer endereço serve aqui, e só o de rede é discado: arquivo **nunca** viaja pelo rádio
    /// ([01, §5](../../../docs/01-visao-e-escopo.md)). Um par pareado pelo Bluetooth é achado na rede
    /// pelo [`Self::localizar`].
    pub destino: Destino,
    /// Onde está o par na rede, quando o destino não diz — ou quando o endereço dele não atende.
    pub localizar: Localizador,
    /// Para contar à interface o que está acontecendo.
    pub avisos: broadcast::Sender<Aviso>,
}

impl std::fmt::Debug for Ajuste {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Ajuste")
            .field("porta", &self.porta)
            .field("recebidos", &self.recebidos)
            .field("destino", &self.destino)
            .finish_non_exhaustive()
    }
}

/// Sobe a tarefa de transferência e devolve por onde pedir envios.
#[must_use]
pub fn iniciar(ajuste: Ajuste) -> Pedidos {
    let (fila, pedidos) = mpsc::unbounded_channel();
    let (destino, mudancas) = watch::channel(ajuste.destino);
    tokio::spawn(servir(ajuste, pedidos, mudancas));
    Pedidos {
        fila,
        destino: Arc::new(destino),
    }
}

/// O laço de vida do canal de dados: tem enlace, usa; não tem, consegue um; o par mudou, recomeça.
async fn servir(
    ajuste: Ajuste,
    mut pedidos: mpsc::UnboundedReceiver<PedidoDeEnvio>,
    mut destino: watch::Receiver<Destino>,
) {
    // Sem par não se abre a porta: seria convidar conexão que nenhuma identidade autorizaria.
    if esperar_par(&ajuste, &mut pedidos, &mut destino)
        .await
        .is_none()
    {
        return;
    }
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
        let Some(par) = esperar_par(&ajuste, &mut pedidos, &mut destino).await else {
            return;
        };
        let alvo = destino.borrow_and_update().alvo;
        let enlace = tokio::select! {
            enlace = obter(&porta, &ajuste, par, alvo) => enlace,
            _ = destino.changed() => continue,
        };
        info!("canal de arquivos estabelecido");
        tokio::select! {
            () = sessao::conduzir(enlace, &ajuste, &mut pedidos) => {}
            _ = destino.changed() => {
                info!("o par mudou; o canal de arquivos recomeça com o novo");
                continue;
            }
        }
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

/// A chave do par, esperando por ela enquanto não há um. `None` quando o serviço está saindo.
///
/// Enquanto espera, cada pedido é recusado com o motivo: um pedido que some deixa o usuário
/// achando que a cópia foi feita.
async fn esperar_par(
    ajuste: &Ajuste,
    pedidos: &mut mpsc::UnboundedReceiver<PedidoDeEnvio>,
    destino: &mut watch::Receiver<Destino>,
) -> Option<PublicKey> {
    let mut avisou = false;
    loop {
        if let Some(chave) = destino.borrow_and_update().chave {
            return Some(chave);
        }
        if !avisou {
            info!("sem par pareado; arquivos indisponíveis até haver um");
            avisou = true;
        }
        // A troca de par primeiro: um pedido feito logo depois de parear chega junto com ela, e
        // sem a ordem o `select!` sorteava — às vezes recusando por "não há par" o que já tinha par.
        tokio::select! {
            biased;
            mudou = destino.changed() => mudou.ok()?,
            pedido = pedidos.recv() => {
                let (caminhos, _) = pedido?;
                recusar(ajuste, &caminhos, Motivo::Outro("não há par pareado".to_owned()));
            }
        }
    }
}

/// Consegue um enlace: atende quem chega, e disca quando é a vez deste lado.
///
/// Os dois lados escutam e os dois podem ter o endereço do outro. Quem disca primeiro é decidido
/// pela regra da chave maior, sem trocar mensagem; o outro só disca depois da carência, para o caso
/// de ser ele o único que sabe o endereço.
async fn obter(
    porta: &Porta,
    ajuste: &Ajuste,
    par: PublicKey,
    alvo: Option<ir_transporte::Endereco>,
) -> EnlaceDeDados {
    let nossa = ajuste.identidade.public();
    let carencia = if ficar_com_o_proprio(nossa, par) {
        Duration::ZERO
    } else {
        CARENCIA_DO_NAO_PREFERIDO
    };

    let configurado = alvo_de_rede(alvo);
    let mut falhou = None;
    loop {
        tokio::select! {
            atendido = porta.aceitar(par) => match atendido {
                Ok(enlace) => return enlace,
                Err(erro) => debug!(%erro, "conexão de arquivos recusada na porta"),
            },
            discado = discar_depois(porta, &ajuste.localizar, (configurado, falhou), par, carencia) => {
                match discado {
                    Ok(enlace) => return enlace,
                    Err(nao_atendeu) => falhou = nao_atendeu,
                }
            }
        }
    }
}

/// Disca depois da carência. O erro diz qual endereço não atendeu (nenhum, se não havia onde
/// discar), para o laço tentar de novo — e perguntar à rede em vez de insistir nele.
///
/// `(configurado, falhou)`: o endereço de rede da configuração, e o último que não atendeu.
async fn discar_depois(
    porta: &Porta,
    localizar: &Localizador,
    (configurado, falhou): (Option<SocketAddr>, Option<SocketAddr>),
    par: PublicKey,
    carencia: Duration,
) -> Result<EnlaceDeDados, Option<SocketAddr>> {
    tokio::time::sleep(carencia).await;
    let Some(alvo) = localizar::onde_discar(localizar, configurado, falhou, par).await else {
        // Ninguém na rede disse onde o par está: ele está desligado, ou longe. Perguntar de novo
        // logo só encheria a rede de broadcast.
        tokio::time::sleep(ESPERA_SEM_ENDERECO).await;
        return Err(None);
    };
    match porta.discar(alvo, par).await {
        Ok(enlace) => Ok(enlace),
        Err(erro) => {
            debug!(%erro, %alvo, "o par ainda não atende no canal de arquivos");
            tokio::time::sleep(ESPERA_ENTRE_TENTATIVAS).await;
            Err(Some(alvo))
        }
    }
}

/// O endereço de rede do par, quando o que se sabe dele é de rede.
const fn alvo_de_rede(alvo: Option<ir_transporte::Endereco>) -> Option<SocketAddr> {
    match alvo {
        Some(ir_transporte::Endereco::Rede(endereco)) => Some(endereco),
        _ => None,
    }
}

/// Responde a todo pedido com a mesma recusa, enquanto o serviço viver.
///
/// Dizer não é melhor que ficar calado: um pedido que some deixa o usuário achando que a cópia foi
/// feita.
async fn recusar_tudo(
    ajuste: &Ajuste,
    pedidos: &mut mpsc::UnboundedReceiver<PedidoDeEnvio>,
    motivo: Motivo,
) {
    while let Some((caminhos, _)) = pedidos.recv().await {
        recusar(ajuste, &caminhos, motivo.clone());
    }
}

/// Conta à interface que este pedido não vai sair, e por quê.
fn recusar(ajuste: &Ajuste, caminhos: &[PathBuf], motivo: Motivo) {
    let nome = caminhos
        .first()
        .and_then(|caminho| caminho.file_name())
        .map_or_else(String::new, |nome| nome.to_string_lossy().into_owned());
    let _ = ajuste.avisos.send(Aviso::Transferencia(Transferencia {
        sentido: Sentido::Enviando,
        nome,
        bytes_feitos: 0,
        bytes_total: 0,
        fase: Fase::Parada(motivo),
    }));
}
