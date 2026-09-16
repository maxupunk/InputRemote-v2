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

mod vocabulario;

#[cfg(test)]
mod testes;

use std::sync::Arc;

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
}

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
                self.receber_entrante(entrante).await;
                return true;
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
            BtCommand::SendFrame(bytes) => self.enviar_quadro(&bytes).await,
            BtCommand::ConfirmPairing(ok) => self.confirmar(ok).await,
            BtCommand::Disconnect => self.derrubar("pedido local"),
            BtCommand::Shutdown => {}
        }
    }

    /// Conecta como iniciador.
    async fn conectar(&mut self, peer: BdAddr, mode: ConnectMode) {
        let canal = match self.radio.conectar(peer).await {
            Ok(canal) => canal,
            Err(erro) => return self.relatar(&erro),
        };
        let mut quadros = Quadros::novo(canal);
        match handshake::conduzir_iniciador(&mut quadros, &self.identity, mode).await {
            Ok(pronto) => self.estabelecer(quadros, peer, pronto),
            Err(erro) => {
                self.relatar(&erro);
                self.derrubar("handshake falhou");
            }
        }
    }

    /// Alguém ligou para esta máquina: responde ao handshake.
    async fn receber_entrante(&mut self, entrante: Result<(R::Canal, BdAddr)>) {
        let (canal, peer) = match entrante {
            Ok(entrante) => entrante,
            Err(erro) => return self.relatar(&erro),
        };
        let mut quadros = Quadros::novo(canal);
        match handshake::conduzir_respondedor(&mut quadros, &self.identity).await {
            Ok(pronto) => self.estabelecer(quadros, peer, pronto),
            Err(erro) => self.relatar(&erro),
        }
    }

    /// Um handshake terminou: ou pede confirmação (pareamento), ou já estabelece (reconexão).
    fn estabelecer(
        &mut self,
        quadros: Quadros<R::Canal>,
        peer: BdAddr,
        pronto: handshake::Established,
    ) {
        let enlace = EnlaceSeguro::novo(quadros, pronto.transport);
        let peer_static = pronto.peer_static;
        if let Some(code) = pronto.code {
            // Pareamento: mostra o código e espera as duas confirmações antes de deixar qualquer
            // quadro de sessão passar.
            self.contar(BtEvent::PairingCode {
                code,
                peer_static,
                peer,
            });
            self.estado = Estado::AguardandoConfirmacao {
                enlace,
                peer_static,
                peer,
                local_ok: false,
                peer_ok: false,
            };
        } else {
            // Reconexão: a identidade já está fixada, então o enlace já vale.
            self.contar(BtEvent::Established { peer_static, peer });
            self.estado = Estado::Estabelecido { enlace };
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

    /// O usuário respondeu à comparação de códigos.
    async fn confirmar(&mut self, ok: bool) {
        let enviado = {
            let Estado::AguardandoConfirmacao {
                enlace, local_ok, ..
            } = &mut self.estado
            else {
                return;
            };
            if ok {
                *local_ok = true;
            }
            let especie = if ok {
                Kind::PairConfirm
            } else {
                Kind::PairReject
            };
            enlace.enviar(especie, &[]).await
        };
        if let Err(erro) = enviado {
            self.relatar(&erro);
        }
        if ok {
            self.promover_se_pronto();
        } else {
            self.derrubar("códigos diferentes");
        }
    }

    /// Estabelece o enlace quando os dois lados confirmaram.
    fn promover_se_pronto(&mut self) {
        if !matches!(
            &self.estado,
            Estado::AguardandoConfirmacao {
                local_ok: true,
                peer_ok: true,
                ..
            }
        ) {
            return;
        }
        let anterior = core::mem::replace(&mut self.estado, Estado::Ocioso);
        if let Estado::AguardandoConfirmacao {
            enlace,
            peer_static,
            peer,
            ..
        } = anterior
        {
            self.contar(BtEvent::Established { peer_static, peer });
            self.estado = Estado::Estabelecido { enlace };
        }
    }

    async fn enviar_quadro(&mut self, bytes: &[u8]) {
        let enviado = {
            let Estado::Estabelecido { enlace } = &mut self.estado else {
                return;
            };
            enlace.enviar(Kind::SessionFrame, bytes).await
        };
        if let Err(erro) = enviado {
            self.relatar(&erro);
            self.derrubar("falha ao enviar");
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
