//! Quando religar o rádio, e o que a janela mostra enquanto isso.
//!
//! O serviço chama [`Vigia::vigiar`] a cada batida com o estado dos enlaces e a hora. O vigia
//! responde se o aviso mudou; o religamento em si roda numa thread própria, porque o sistema leva
//! segundos para desligar e ligar o rádio, e o ator do serviço gira a cada 5 ms.

use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::time::{Duration, Instant};

use ir_ipc::{BluetoothParado, Falha, Resposta};
use tracing::{info, warn};

use crate::Adaptador;

/// Quanto tempo o Bluetooth fica fora, com a rede de pé, antes de o rádio ser religado.
///
/// Uma queda comum volta antes disso: a sessão anterior que o kernel ainda segura solta em ~20 s
/// (log 28), e a rediscagem vem logo depois. Um minuto separa o rádio travado do soluço.
pub const PRAZO: Duration = Duration::from_secs(60);

/// Os enlaces com o par, como o serviço os vê agora.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Enlaces {
    /// Se o Bluetooth deveria estar de pé: há rádio aqui, o do par é conhecido, e a pessoa não
    /// fixou a rede.
    pub radio_esperado: bool,
    /// Se o enlace pelo Bluetooth está de pé.
    pub radio: bool,
    /// Se o enlace pela rede está de pé — a prova de que o par está ligado e com o serviço no ar.
    pub rede: bool,
}

/// Quem pediu o religamento.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Modo {
    /// O próprio vigia: só religa sem outro aparelho conectado.
    Sozinho,
    /// A pessoa, pelo botão: ela já leu o custo na faixa.
    APedido,
}

/// Como terminou um religamento.
#[derive(Debug)]
enum Desfecho {
    Religado,
    OutrosAparelhos,
    Falhou(String),
}

/// O vigia do rádio: decide quando religar e diz o que a janela mostra.
pub struct Vigia {
    adaptador: Arc<dyn Adaptador>,
    /// Desde quando o Bluetooth está fora com a rede de pé.
    fora_desde: Option<Instant>,
    /// Se já se religou nesta queda: uma vez só, até o Bluetooth voltar.
    tentou: bool,
    situacao: Option<BluetoothParado>,
    /// A última situação que [`Self::vigiar`] contou como mudança.
    anunciada: Option<BluetoothParado>,
    /// O religamento em curso, quando há.
    em_curso: Option<Receiver<Desfecho>>,
}

impl std::fmt::Debug for Vigia {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Vigia")
            .field("fora_desde", &self.fora_desde)
            .field("tentou", &self.tentou)
            .field("situacao", &self.situacao)
            .field("em_curso", &self.em_curso.is_some())
            .finish_non_exhaustive()
    }
}

impl Default for Vigia {
    fn default() -> Self {
        Self::new(crate::do_sistema())
    }
}

impl Vigia {
    /// Um vigia sobre este adaptador.
    #[must_use]
    pub fn new(adaptador: Arc<dyn Adaptador>) -> Self {
        Self {
            adaptador,
            fora_desde: None,
            tentou: false,
            situacao: None,
            anunciada: None,
            em_curso: None,
        }
    }

    /// O que a janela mostra agora, se o Bluetooth parou.
    #[must_use]
    pub const fn situacao(&self) -> Option<BluetoothParado> {
        self.situacao
    }

    /// Olha os enlaces e decide. Devolve se a situação mudou desde a última vez que devolveu
    /// `true` — inclusive por [`Self::religar_agora`] —, para o serviço avisar a janela.
    pub fn vigiar(&mut self, enlaces: Enlaces, agora: Instant) -> bool {
        self.colher(agora);
        if enlaces.radio {
            self.fora_desde = None;
            self.tentou = false;
            self.limpar();
        } else if enlaces.radio_esperado && enlaces.rede {
            let desde = *self.fora_desde.get_or_insert(agora);
            if self.em_curso.is_none() && agora.saturating_duration_since(desde) >= PRAZO {
                self.venceu();
            }
        } else {
            // Sem rede não há prova de que o par está lá, e sem rádio esperado não há o que
            // religar: o aviso da sessão é outro.
            self.fora_desde = None;
            self.limpar();
        }
        let mudou = self.situacao != self.anunciada;
        self.anunciada = self.situacao;
        mudou
    }

    /// O botão da janela: religar agora, mesmo com outros aparelhos conectados.
    ///
    /// A resposta é a que a janela recebe: não começa sem suporte nem com outro religamento em
    /// curso.
    pub fn religar_agora(&mut self) -> Resposta {
        if !self.adaptador.sabe_religar() || self.em_curso.is_some() {
            return Resposta::Falha(Falha::SemBluetooth);
        }
        info!("religando o rádio Bluetooth a pedido da janela");
        self.comecar(Modo::APedido);
        Resposta::Feito
    }

    /// Tira o aviso, a menos que um religamento esteja em curso: ele termina e conta como foi.
    fn limpar(&mut self) {
        if self.em_curso.is_none() {
            self.situacao = None;
        }
    }

    /// O Bluetooth está fora há [`PRAZO`], com a rede de pé.
    fn venceu(&mut self) {
        if !self.adaptador.sabe_religar() {
            self.situacao = Some(BluetoothParado::SemReligar);
        } else if !self.tentou {
            info!(
                "o Bluetooth não alcança o par há {} s, e a rede alcança: religando o rádio daqui",
                PRAZO.as_secs()
            );
            self.comecar(Modo::Sozinho);
        } else if matches!(self.situacao, None | Some(BluetoothParado::Religando)) {
            warn!("o Bluetooth não voltou depois de religar o rádio daqui");
            self.situacao = Some(BluetoothParado::NaoResolveu);
        }
    }

    fn comecar(&mut self, modo: Modo) {
        self.tentou = true;
        self.situacao = Some(BluetoothParado::Religando);
        let (envio, recebimento) = mpsc::channel();
        let adaptador = Arc::clone(&self.adaptador);
        let lancado = std::thread::Builder::new()
            .name("religar-bluetooth".to_owned())
            .spawn(move || {
                let _ = envio.send(executar(adaptador.as_ref(), modo));
            });
        match lancado {
            Ok(_) => self.em_curso = Some(recebimento),
            Err(erro) => {
                warn!(%erro, "não foi possível começar a religar o rádio Bluetooth");
                self.situacao = Some(BluetoothParado::SemReligar);
            }
        }
    }

    /// Recolhe o desfecho do religamento em curso, se ele já terminou.
    fn colher(&mut self, agora: Instant) {
        let Some(recebimento) = &self.em_curso else {
            return;
        };
        let desfecho = match recebimento.try_recv() {
            Ok(desfecho) => desfecho,
            Err(TryRecvError::Empty) => return,
            Err(TryRecvError::Disconnected) => {
                Desfecho::Falhou("o religamento parou sem resposta".to_owned())
            }
        };
        self.em_curso = None;
        match desfecho {
            Desfecho::Religado => {
                // O aviso segue em "Religando" até o par voltar, ou até um prazo novo vencer.
                info!("rádio Bluetooth religado; esperando o par");
                self.fora_desde = Some(agora);
            }
            Desfecho::OutrosAparelhos => {
                self.situacao = Some(BluetoothParado::ComOutrosAparelhos);
            }
            Desfecho::Falhou(motivo) => {
                warn!(%motivo, "não foi possível religar o rádio Bluetooth");
                self.situacao = Some(BluetoothParado::SemReligar);
            }
        }
    }
}

/// O religamento, fora do ator.
fn executar(adaptador: &dyn Adaptador, modo: Modo) -> Desfecho {
    if modo == Modo::Sozinho {
        match adaptador.aparelhos_conectados() {
            Ok(0) => {}
            Ok(quantos) => {
                info!(
                    quantos,
                    "outros aparelhos Bluetooth conectados: religar fica com a pessoa"
                );
                return Desfecho::OutrosAparelhos;
            }
            // Sem saber, não se derruba nada: a pessoa decide pelo botão.
            Err(motivo) => {
                warn!(%motivo, "não deu para saber quem mais está no rádio");
                return Desfecho::OutrosAparelhos;
            }
        }
    }
    match adaptador.religar() {
        Ok(()) => Desfecho::Religado,
        Err(motivo) => Desfecho::Falhou(motivo),
    }
}

#[cfg(test)]
mod testes;
