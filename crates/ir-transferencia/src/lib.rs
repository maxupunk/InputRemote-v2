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

mod despejo;
mod desvio;
mod enlace;
mod enviando;
pub mod faxina;
mod fila;
mod localizar;
mod passo;
mod pedidos;
mod porta;
mod recebendo;
mod retomada;
mod sessao;

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use ir_crypto::{Identity, PublicKey};
/// A cota de quem recebe, reexportada para quem monta o [`Ajuste`] não precisar de `ir-files`.
///
/// Quem configura a transferência não tem por que conhecer o motor dela
/// ([02, §2](../../../docs/02-arquitetura.md)).
pub use ir_files::Cota;

pub use desvio::{EstadoDaFaixa, Faixa};
/// Com a autoridade de quem os arquivos são lidos, reexportada pelo mesmo motivo de [`Cota`].
pub use ir_files::{Autorizacao, Leitor};
pub use localizar::{Localizador, da_descoberta, sem_localizador};
pub use pedidos::Pedidos;

/// Um pedido de envio: o que mandar, e com a autoridade de quem.
pub(crate) type PedidoDeEnvio = (Vec<PathBuf>, Leitor);
use ir_ipc::Aviso;
use ir_ipc::transferencia::{Fase, Motivo, Sentido};
use tokio::sync::{broadcast, mpsc, watch};
use tracing::{info, warn};

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

impl Destino {
    /// O destino pelo que a configuração diz: a chave fixada, e o endereço configurado à mão — que
    /// vence — ou, sem ele, o endereço por onde o par foi pareado.
    #[must_use]
    pub fn da_configuracao(
        chave: Option<PublicKey>,
        configurado: Option<&str>,
        gravado: Option<&str>,
    ) -> Self {
        Self {
            chave,
            alvo: configurado
                .or(gravado)
                .and_then(ir_transporte::Endereco::ler),
        }
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
    let (acorda, toques) = mpsc::unbounded_channel();
    let (destino, mudancas) = watch::channel(ajuste.destino);
    let fila = Arc::new(fila::Fila::default());
    let faxineiro = faxina::Faxineiro::novo(ajuste.recebidos.clone(), faxina::Limites::default());
    let de_pe = Arc::new(AtomicBool::new(false));
    let trafego = Arc::new(ir_transporte::dados::Contador::default());
    let avisos = ajuste.avisos.clone();
    let entrada = Entrada {
        fila: Arc::clone(&fila),
        toques,
    };
    let (desvio, faixa) = desvio::Desvio::novo();
    let deposito = recebendo::Deposito {
        pasta: ajuste.recebidos.clone(),
        cota: ajuste.cota,
        faxineiro: Arc::clone(&faxineiro),
        recepcoes: Arc::default(),
        desvio: desvio.clone(),
    };
    let canal = (entrada, deposito, Arc::clone(&de_pe), Arc::clone(&trafego));
    tokio::spawn(servir(ajuste, canal, mudancas));
    Pedidos {
        fila,
        faxineiro,
        acorda,
        destino: Arc::new(destino),
        de_pe,
        avisos,
        desvio,
        faixa: Arc::new(std::sync::Mutex::new(Some(faixa))),
        trafego,
    }
}

/// O que a tarefa do canal recebe de quem a sobe.
type Canal = (
    Entrada,
    recebendo::Deposito,
    Arc<AtomicBool>,
    Arc<ir_transporte::dados::Contador>,
);

/// Por onde os pedidos chegam à tarefa de envio.
///
/// A fila guarda **o que** copiar; o canal só acorda quem espera — e, ao fechar, diz que o serviço
/// está saindo. Juntos porque um sem o outro não serve: a fila sozinha não avisa, e o canal sozinho
/// não guarda.
pub(crate) struct Entrada {
    pub(crate) fila: Arc<fila::Fila>,
    toques: mpsc::UnboundedReceiver<()>,
}

impl Entrada {
    /// Espera um toque. `None` quando o serviço está saindo.
    pub(crate) async fn esperar(&mut self) -> Option<()> {
        self.toques.recv().await
    }
}

/// O laço de vida do canal de dados: tem enlace, usa; não tem, consegue um; o par mudou, recomeça.
///
/// `canal`: por onde os pedidos chegam, para onde o que chega vai, a marca de enlace de pé que
/// quem pede consulta, e o contador do que o canal move.
async fn servir(
    ajuste: Ajuste,
    (mut entrada, deposito, de_pe, trafego): Canal,
    mut destino: watch::Receiver<Destino>,
) {
    let faxineiro = &deposito.faxineiro;
    // Ao subir, antes de qualquer coisa: a pasta começa vazia. O que ficou de uma sessão anterior
    // é de uma cópia que já foi colada ou já foi esquecida — o clipboard não sobrevive ao
    // desligamento, então ninguém vai colar aquilo. Guardar é só ocupar disco. Durante a sessão a
    // pasta se cuida pela política de `faxina`, que protege o que acabou de chegar. As montagens
    // que um serviço morto deixou saem também: agora nenhuma cópia está em curso.
    ir_files::staging::recolher_orfas(faxineiro.pasta()).await;
    faxineiro.esvaziar().await;
    // Sem par não se abre a porta: seria convidar conexão que nenhuma identidade autorizaria.
    if esperar_par(&ajuste, &mut entrada, &mut destino)
        .await
        .is_none()
    {
        return;
    }
    let Some(porta) = porta::abrir_a_porta(&ajuste, &mut entrada, trafego).await else {
        return;
    };
    info!(porta = ajuste.porta, "canal de arquivos no ar");

    loop {
        let Some(par) = esperar_par(&ajuste, &mut entrada, &mut destino).await else {
            return;
        };
        let alvo = destino.borrow_and_update().alvo;
        let enlace = tokio::select! {
            enlace = enlace::obter(&porta, &ajuste, par, alvo) => enlace,
            _ = destino.changed() => continue,
        };
        info!("canal de arquivos estabelecido");
        de_pe.store(true, Ordering::Release);
        let mudou = tokio::select! {
            () = sessao::conduzir(enlace, &ajuste, &mut entrada, &deposito) => false,
            _ = destino.changed() => true,
        };
        de_pe.store(false, Ordering::Release);
        if mudou {
            info!("o par mudou; o canal de arquivos recomeça com o novo");
            // Quem conduzia foi largado no meio, sem dizer como a cópia em curso terminou.
            if let Some(trabalho) = entrada.fila.abandonada() {
                let nome = ir_files::publicacao::nome_do_pedido(&trabalho.caminhos);
                let progresso = trabalho.progresso;
                retomada::envio_caiu(&entrada.fila, &ajuste.avisos, trabalho, (&nome, progresso));
            }
        } else {
            warn!("o canal de arquivos caiu; teclado e mouse não foram afetados");
        }
        // A cópia que caiu no meio voltou para a fila (`retomada::envio_caiu`); a pedida no
        // instante da queda também espera ali. As duas aparecem na tela esperando a conexão, com
        // prazo — e não caladas até ela voltar.
        retomada::mostrar_espera(&entrada.fila, &ajuste.avisos);
    }
}

/// A chave do par, esperando por ela enquanto não há um. `None` quando o serviço está saindo.
///
/// Enquanto espera, cada pedido é recusado com o motivo: um pedido que some deixa o usuário
/// achando que a cópia foi feita.
async fn esperar_par(
    ajuste: &Ajuste,
    entrada: &mut Entrada,
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
        let mut mudou = Ok(());
        let motivo = Motivo::Outro("não há par pareado".to_owned());
        let troca = async { mudou = destino.changed().await };
        recusar_enquanto(ajuste, entrada, troca, &motivo).await?;
        mudou.ok()?;
    }
}

/// Conta à interface que este pedido não vai sair, e por quê.
///
/// Dizer não é melhor que ficar calado: um pedido que some deixa o usuário achando que a cópia foi
/// feita. O nome é o que a entrega teria ([`ir_files::publicacao::nome_do_pedido`]), para a recusa
/// e a cópia que desse certo falarem da mesma coisa.
pub(crate) fn recusar(ajuste: &Ajuste, caminhos: &[PathBuf], motivo: Motivo) {
    let nome = ir_files::publicacao::nome_do_pedido(caminhos);
    let fase = Fase::Parada(motivo);
    sessao::anunciar(&ajuste.avisos, Sentido::Enviando, &nome, (0, 0), fase);
}

/// Espera o `tempo` passar recusando, com `motivo`, todo pedido que chegar nesse meio.
///
/// É o que o canal faz enquanto não pode trabalhar — sem par, sem porta. `None` quando o serviço
/// está saindo.
pub(crate) async fn recusar_enquanto(
    ajuste: &Ajuste,
    entrada: &mut Entrada,
    tempo: impl std::future::Future<Output = ()>,
    motivo: &Motivo,
) -> Option<()> {
    tokio::pin!(tempo);
    loop {
        tokio::select! {
            biased;
            () = &mut tempo => return Some(()),
            toque = entrada.esperar() => {
                toque?;
                if let Some(trabalho) = entrada.fila.descartar() {
                    recusar(ajuste, &trabalho.caminhos, motivo.clone());
                }
            }
        }
    }
}
