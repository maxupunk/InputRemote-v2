//! O endpoint RFCOMM: uma tarefa dona do rádio, que fala com o serviço por canais.
//!
//! Mesmo papel do endpoint UDP do `ir-net`: converte comando em bytes no canal e bytes do canal
//! em evento. Ele não conhece a máquina de estados da sessão; move quadros cifrados e coordena o
//! pareamento. O vocabulário da fronteira está em [`vocabulario`].
//!
//! # O que este módulo garante
//!
//! - **Nada de sessão trafega antes das duas confirmações de pareamento.** Só há
//!   [`BtEvent::Established`] quando o usuário local confirmou **e** o par confirmou
//!   ([04, §3.2](../../../../docs/04-seguranca.md)).
//! - **Um quadro que não abre derruba o enlace.** É o oposto do UDP, e o motivo está abaixo.
//!
//! # Por que aqui um quadro ruim derruba, e no UDP não
//!
//! No UDP, um datagrama que não abre é ignorado: ele pode ser lixo, repetição ou de outra
//! sessão, e derrubar o enlace por um pacote solto seria negação de serviço trivial — qualquer
//! um manda um pacote.
//!
//! No RFCOMM não existe "pacote solto". O meio é confiável e ordenado, e o contador é contado
//! nas duas pontas ([`link`](crate::link)). Um quadro que não abre significa que a contagem
//! divergiu, e todos os quadros seguintes vão falhar também. Ignorar deixaria o enlace vivo e
//! mudo — o pior estado possível, porque nem funciona nem cai. Derrubar faz a reconexão começar.
//!
//! # Erro e queda são eventos diferentes
//!
//! Uma falha costuma produzir os dois: [`BtEvent::Error`] leva a frase que a tela mostra, com o
//! que a pessoa pode fazer a respeito; [`BtEvent::LinkDown`] leva o motivo curto e estável de
//! que a máquina de estados precisa. Misturá-los obrigaria a interface a interpretar texto, ou
//! deixaria o usuário sem explicação — e o registro sem a causa.

mod confirmacao;
mod entrante;
mod vocabulario;

#[cfg(test)]
mod testes;

use std::sync::Arc;
use std::time::Duration;

use ir_crypto::{Identity, PublicKey};
use tokio::sync::mpsc;

pub use self::vocabulario::{BtCommand, BtEvent};
use crate::addr::BdAddr;
use crate::canal::Quadros;
use crate::error::{BtError, Result};
use crate::handshake::{self, ConnectMode};
use crate::link::EnlaceSeguro;
use crate::radio::Radio;
use crate::wire::Kind;

/// A partir de quanto tempo na fila um quadro não vale mais o rádio.
///
/// Um quarto do prazo de queda da sessão (1 s), que é também o teto do intervalo de retransmissão
/// ([03, §4.1](../../../../docs/03-protocolo.md)): um quadro confiável que esperou isso já tem uma
/// retransmissão a caminho, e uma amostra de ponteiro dessa idade já foi superada por outra. Na
/// rota dupla, a cópia pela rede chegou faz tempo. Mandar mesmo assim só atrasaria o que é novo.
pub const VELHO_DEMAIS: Duration = Duration::from_millis(250);

/// Estado interno do endpoint.
#[derive(Debug)]
enum Estado<C> {
    Ocioso,
    AguardandoConfirmacao {
        enlace: EnlaceSeguro<C>,
        peer_static: PublicKey,
        peer: BdAddr,
        local_ok: bool,
        peer_ok: bool,
    },
    /// Com enlace de pé. O endereço do par não fica guardado aqui: quem redisca é o serviço, que
    /// o tem na configuração, e estado que ninguém lê é estado que sai de sincronia calado.
    Estabelecido {
        enlace: EnlaceSeguro<C>,
    },
}

/// O que uma volta do laço produziu.
enum Volta {
    Comando(Option<BtCommand>),
    Recebido(Result<(Kind, Vec<u8>)>),
}

/// Alças para falar com um endpoint em execução.
#[derive(Debug)]
pub struct EndpointHandle {
    /// Manda comandos ao endpoint.
    pub commands: mpsc::UnboundedSender<BtCommand>,
    /// Recebe eventos do endpoint.
    pub events: mpsc::UnboundedReceiver<BtEvent>,
}

/// O endpoint em si.
pub struct Endpoint<R: Radio> {
    radio: Arc<R>,
    identity: Arc<Identity>,
    events: mpsc::UnboundedSender<BtEvent>,
    estado: Estado<R::Canal>,
    /// Quantas reconexões foram pedidas desde o último enlace, para a vez de discar
    /// ([`ir_crypto::turno`]).
    rodadas: u32,
    /// Quantos quadros velhos demais foram descartados desde o último registro.
    descartados: u64,
    /// Se um pareamento que chega de fora é atendido ([`BtCommand::AcceptPairing`]).
    aceitar_pareamento: bool,
    /// Quantas vezes seguidas a escuta falhou.
    falhas_da_escuta: u32,
}

/// Quantas falhas seguidas da escuta fazem o rádio ser dado como perdido.
const FALHAS_ATE_PERDER_O_RADIO: u32 = 3;

/// Quanto uma escrita no rádio pode demorar antes de o enlace ser dado como perdido.
///
/// Sem prazo, um rádio que parou de escoar travava o endpoint inteiro na escrita: nem comando nem
/// quadro recebido eram atendidos, e a fila crescia atrás. Derrubar deixa a sessão seguir pela rede
/// e o rádio voltar na próxima discagem.
pub const PRAZO_DA_ESCRITA: std::time::Duration = std::time::Duration::from_secs(2);

impl<R: Radio> core::fmt::Debug for Endpoint<R> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Endpoint").finish_non_exhaustive()
    }
}

impl<R: Radio> Endpoint<R> {
    /// Sobe o endpoint numa tarefa e devolve as alças para conversar com ele.
    #[must_use]
    pub fn spawn(radio: Arc<R>, identity: Arc<Identity>) -> EndpointHandle {
        let (cmd_tx, cmd_rx) = mpsc::unbounded_channel();
        let (evt_tx, evt_rx) = mpsc::unbounded_channel();
        let endpoint = Self {
            radio,
            identity,
            events: evt_tx,
            estado: Estado::Ocioso,
            rodadas: 0,
            descartados: 0,
            aceitar_pareamento: true,
            falhas_da_escuta: 0,
        };
        tokio::spawn(endpoint.rodar(cmd_rx));
        EndpointHandle {
            commands: cmd_tx,
            events: evt_rx,
        }
    }

    async fn rodar(mut self, mut comandos: mpsc::UnboundedReceiver<BtCommand>) {
        // Ocioso, escuta-se quem liga. Com enlace de pé, lê-se o enlace: um canal por vez, que é
        // o que o portador comporta.
        loop {
            let continuar = if matches!(self.estado, Estado::Ocioso) {
                self.volta_ocioso(&mut comandos).await
            } else {
                self.volta_ligado(&mut comandos).await
            };
            if !continuar {
                break;
            }
        }
    }

    /// Uma volta sem enlace: ou chega comando, ou alguém liga. Devolve se o laço continua.
    async fn volta_ocioso(&mut self, comandos: &mut mpsc::UnboundedReceiver<BtCommand>) -> bool {
        // O `Arc` é clonado para a espera não segurar `self` emprestado enquanto o outro ramo
        // precisa mudá-lo.
        let radio = Arc::clone(&self.radio);
        let volta = tokio::select! {
            comando = comandos.recv() => Volta::Comando(comando),
            entrante = radio.aceitar() => {
                return self.receber_entrante(entrante).await;
            }
        };
        self.agir(volta).await
    }

    /// Uma volta com enlace: ou chega comando, ou chega quadro.
    async fn volta_ligado(&mut self, comandos: &mut mpsc::UnboundedReceiver<BtCommand>) -> bool {
        let volta = {
            let enlace = match &mut self.estado {
                Estado::AguardandoConfirmacao { enlace, .. } | Estado::Estabelecido { enlace } => {
                    enlace
                }
                Estado::Ocioso => return true,
            };
            tokio::select! {
                comando = comandos.recv() => Volta::Comando(comando),
                recebido = enlace.receber() => Volta::Recebido(recebido),
            }
        };
        self.agir(volta).await
    }

    /// O que fazer com o que a volta produziu. Devolve se o laço continua.
    async fn agir(&mut self, volta: Volta) -> bool {
        match volta {
            Volta::Comando(None | Some(BtCommand::Shutdown)) => false,
            Volta::Comando(Some(comando)) => {
                self.executar(comando).await;
                true
            }
            Volta::Recebido(recebido) => {
                self.receber_quadro(recebido);
                true
            }
        }
    }

    async fn executar(&mut self, comando: BtCommand) {
        match comando {
            BtCommand::Connect { peer, mode } => self.conectar(peer, mode).await,
            BtCommand::SendFrame { bytes, queued_at } => {
                if queued_at.elapsed() > VELHO_DEMAIS {
                    self.descartar_velho();
                } else {
                    self.enviar_quadro(&bytes).await;
                }
            }
            BtCommand::ConfirmPairing(ok) => self.confirmar(ok).await,
            BtCommand::AcceptPairing(aceitar) => self.aceitar_pareamento = aceitar,
            BtCommand::Disconnect => self.derrubar("pedido local"),
            BtCommand::Shutdown => {}
        }
    }

    /// Conecta como iniciador — na reconexão, só se for a vez deste lado.
    ///
    /// Com a rota dupla os dois lados ficam sabendo o endereço de rádio do outro no mesmo instante
    /// e discam juntos. Discando os dois, cada um espera a resposta de quem também está só
    /// discando, e o aperto de mão vence o prazo dos dois lados. A regra de quem disca é a mesma da
    /// rede ([`ir_crypto::turno`]).
    async fn conectar(&mut self, peer: BdAddr, mode: ConnectMode) {
        if let ConnectMode::Reconnect(chave_do_par) = mode {
            self.rodadas = self.rodadas.wrapping_add(1);
            if !ir_crypto::turno::discar_nesta_rodada(
                self.identity.public(),
                chave_do_par,
                self.rodadas,
            ) {
                return;
            }
        }
        let canal = match self.radio.conectar(peer).await {
            Ok(canal) => canal,
            Err(erro) => return self.relatar(&erro),
        };
        let mut quadros = Quadros::novo(canal);
        match handshake::conduzir_iniciador(&mut quadros, &self.identity, mode).await {
            Ok(pronto) => self.estabelecer(quadros, peer, pronto, false),
            Err(erro) => {
                self.relatar(&erro);
                self.derrubar("handshake falhou");
            }
        }
    }

    /// Chegou um quadro pelo enlace.
    fn receber_quadro(&mut self, recebido: Result<(Kind, Vec<u8>)>) {
        let (especie, conteudo) = match recebido {
            Ok(aberto) => aberto,
            Err(erro) => {
                // Sobre um meio confiável e ordenado, isto não é perda. São duas causas
                // diferentes, e registrar uma pela outra esconde justamente o que aconteceu: ou
                // o par foi embora, ou os bytes chegaram adulterados.
                let motivo = if matches!(erro, BtError::SemResposta) {
                    "o par encerrou o canal"
                } else {
                    "o quadro não abriu"
                };
                self.relatar(&erro);
                self.derrubar(motivo);
                return;
            }
        };
        match especie {
            Kind::SessionFrame => self.entregar(conteudo),
            Kind::PairConfirm => {
                if let Estado::AguardandoConfirmacao { peer_ok, .. } = &mut self.estado {
                    *peer_ok = true;
                }
                self.promover_se_pronto();
            }
            Kind::PairReject => self.derrubar("o par recusou o pareamento"),
        }
    }

    fn entregar(&mut self, conteudo: Vec<u8>) {
        if matches!(self.estado, Estado::Estabelecido { .. }) {
            self.contar(BtEvent::Frame(conteudo));
        }
        // Quadro de sessão antes da confirmação é descartado: nada de sessão trafega antes das
        // duas confirmações.
    }

    async fn enviar_quadro(&mut self, bytes: &[u8]) {
        let enviado = {
            let Estado::Estabelecido { enlace } = &mut self.estado else {
                return;
            };
            tokio::time::timeout(PRAZO_DA_ESCRITA, enlace.enviar(Kind::SessionFrame, bytes)).await
        };
        match enviado {
            Ok(Ok(())) => {}
            Ok(Err(erro)) => {
                self.relatar(&erro);
                self.derrubar("falha ao enviar");
            }
            Err(_) => self.derrubar("o rádio parou de escoar"),
        }
    }

    /// Um quadro esperou demais na fila: não vale mais o rádio.
    ///
    /// Conta, e registra na primeira de uma rajada e depois a cada cem — sob interferência são
    /// dezenas por segundo, e uma linha por quadro afogaria o registro.
    fn descartar_velho(&mut self) {
        self.descartados = self.descartados.saturating_add(1);
        if self.descartados % 100 == 1 {
            tracing::debug!(
                descartados = self.descartados,
                "quadros velhos demais descartados antes do rádio"
            );
        }
    }

    fn derrubar(&mut self, motivo: &'static str) {
        if !matches!(self.estado, Estado::Ocioso) {
            self.estado = Estado::Ocioso;
            self.contar(BtEvent::LinkDown(motivo));
        }
    }

    /// Conta um erro ao serviço, com o que o usuário pode fazer quando há o que fazer.
    fn relatar(&self, erro: &BtError) {
        self.contar(BtEvent::Error(descrever(erro)));
    }

    fn contar(&self, evento: BtEvent) {
        let _ = self.events.send(evento);
    }
}

/// O erro em texto, com a instrução ao usuário quando ela existe.
fn descrever(erro: &BtError) -> String {
    match erro.o_que_fazer() {
        Some(instrucao) => format!("{erro} — {instrucao}"),
        None => erro.to_string(),
    }
}
