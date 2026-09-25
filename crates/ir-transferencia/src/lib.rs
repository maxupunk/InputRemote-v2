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
mod enlace;
mod enviando;
pub mod faxina;
mod fila;
mod localizar;
mod passo;
mod porta;
mod recebendo;
mod sessao;

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
pub use localizar::{Localizador, da_descoberta, sem_localizador};

/// Um pedido de envio: o que mandar, e com a autoridade de quem.
pub(crate) type PedidoDeEnvio = (Vec<PathBuf>, Leitor);
use ir_ipc::Aviso;
use ir_ipc::transferencia::{Fase, Motivo, Sentido};
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

/// Por onde o ator pede um envio.
#[derive(Debug, Clone)]
pub struct Pedidos {
    /// A fila de um: a mesma cópia não vai duas vezes, e outra cópia substitui a que está indo.
    fila: Arc<fila::Fila>,
    /// Quem cuida da pasta de recebidos.
    faxineiro: Arc<faxina::Faxineiro>,
    /// O toque que acorda a tarefa de envio. Fechá-lo é como o serviço diz que está saindo.
    acorda: mpsc::UnboundedSender<()>,
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
        let (acorda, _) = mpsc::unbounded_channel();
        let (destino, _) = watch::channel(Destino::default());
        Self {
            fila: Arc::new(fila::Fila::default()),
            faxineiro: faxina::Faxineiro::novo(PathBuf::new(), faxina::Limites::default()),
            acorda,
            destino: Arc::new(destino),
        }
    }

    /// Esvazia a pasta de recebidos e conta o estado novo quando terminar.
    ///
    /// Apagar gigabytes toca disco e pode demorar; quem pede é o ator do serviço, que gira a cada
    /// 5 ms e não pode esperar. Então isto manda fazer e volta na hora: o tamanho de volta chega
    /// pelo aviso, que é como toda mudança de estado chega à janela.
    pub fn limpar_recebidos(&self, avisos: &broadcast::Sender<Aviso>, estado: ir_ipc::Estado) {
        let faxineiro = Arc::clone(&self.faxineiro);
        let avisos = avisos.clone();
        tokio::spawn(async move {
            faxineiro.esvaziar().await;
            let _ = avisos.send(Aviso::EstadoMudou(ir_ipc::Estado {
                recebidos_bytes: faxineiro.espaco(),
                ..estado
            }));
        });
    }

    /// Quem cuida da pasta de recebidos: quanto ela ocupa, e o pedido de esvaziá-la.
    ///
    /// A janela mostra o tamanho e oferece o botão; o serviço não decide por conta própria apagar
    /// o que o usuário ainda não colou — fora dos limites de [`faxina::Limites`], que são a parte
    /// automática.
    #[must_use]
    pub fn recebidos(&self) -> &Arc<faxina::Faxineiro> {
        &self.faxineiro
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

    /// Para a cópia em curso e esquece a que esperava. `false` se não havia cópia.
    ///
    /// O outro lado recebe o cancelamento e apaga o que já gravou: a montagem dele só vira arquivo
    /// no fim.
    #[must_use]
    pub fn cancelar(&self) -> bool {
        self.fila.cancelar_tudo()
    }

    /// Pede o envio destes caminhos, lidos com a autoridade de `leitor`. `false` quando a
    /// transferência não está de pé.
    ///
    /// `leitor` não é detalhe: o serviço tem mais autoridade que quem pede, e sem ele o serviço leria
    /// **por** quem pediu o que essa pessoa não leria sozinha (`ir_files::permissao`).
    ///
    /// Caminho vazio é descartado, e um pedido que fica sem nenhum é recusado aqui mesmo: é o único
    /// erro que se vê sem tocar o disco.
    /// Pedir de novo a **mesma** cópia que já está indo é aceito e não vira outra: é o Ctrl+C
    /// repetido de quem não viu retorno na tela. Pedir outra cancela a que está indo
    /// ([`fila::Fila`]).
    #[must_use]
    pub fn enviar(&self, caminhos: Vec<PathBuf>, leitor: Leitor) -> bool {
        let caminhos: Vec<PathBuf> = caminhos
            .into_iter()
            .filter(|caminho| !caminho.as_os_str().is_empty())
            .collect();
        if caminhos.is_empty() {
            return false;
        }
        let recebido = self.fila.pedir(caminhos, leitor);

        if recebido == fila::Recebido::Repetido {
            debug!("esta cópia já está indo; não vai de novo");
            return true;
        }
        self.acorda.send(()).is_ok()
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
    let entrada = Entrada {
        fila: Arc::clone(&fila),
        toques,
    };
    tokio::spawn(servir(ajuste, entrada, mudancas, Arc::clone(&faxineiro)));
    Pedidos {
        fila,
        faxineiro,
        acorda,
        destino: Arc::new(destino),
    }
}

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
async fn servir(
    ajuste: Ajuste,
    mut entrada: Entrada,
    mut destino: watch::Receiver<Destino>,
    faxineiro: Arc<faxina::Faxineiro>,
) {
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
    let Some(porta) = porta::abrir_a_porta(&ajuste, &mut entrada).await else {
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
        tokio::select! {
            () = sessao::conduzir(enlace, &ajuste, &mut entrada, &faxineiro) => {}
            _ = destino.changed() => {
                info!("o par mudou; o canal de arquivos recomeça com o novo");
                continue;
            }
        }
        // O aviso à tela, se havia cópia atravessando, já saiu de quem a conduzia
        // (`sessao::anunciar_queda`): só ali se sabe o nome dela, e se havia uma.
        warn!("o canal de arquivos caiu; teclado e mouse não foram afetados");
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
