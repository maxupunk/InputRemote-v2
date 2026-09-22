//! O lado receptor: ordem, repetição e confirmação a enviar.
//!
//! Duas responsabilidades, e as duas existem pelo mesmo motivo — evitar tecla presa.
//!
//! **Repetição.** Um datagrama pode chegar duas vezes, por retransmissão nossa, por caminho
//! múltiplo na rede, ou porque alguém gravou e reenviou. Aplicar duas vezes um `KeyDown`
//! digitaria a tecla duas vezes (`docs/04-seguranca.md` §2).
//!
//! **Ordem.** É a parte menos óbvia e a mais perigosa. A retransmissão **cria** entrega fora
//! de ordem: se um `KeyDown` se perde e o `KeyUp` seguinte chega inteiro, o `KeyDown`
//! retransmitido chega **depois** do `KeyUp` — e a tecla fica pressionada para sempre, porque
//! o evento que a soltaria já passou. Por isso o canal confiável entrega em ordem: o que
//! chega adiantado espera na fila até o buraco ser preenchido.

use std::collections::BTreeMap;

use ir_proto::frame::{Ack, Frame, Sequence};

/// Quantos quadros adiantados podem esperar na fila.
///
/// Igual à janela do emissor: ele nunca manda mais que isso sem confirmação, então uma fila
/// maior guardaria o que nunca vai chegar. Estourar o limite significa que o enlace perdeu
/// mais do que consegue reparar, e a resposta é derrubá-lo.
pub(crate) const REORDER_LIMIT: usize = crate::sender::WINDOW;

/// O que fazer com um quadro recebido.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Delivery {
    /// Já visto, ou anterior à janela. Descarte sem processar.
    Duplicate,
    /// Chegou adiantado. Está guardado até o buraco à frente ser preenchido.
    Buffered,
    /// Entregue estes quadros, nesta ordem.
    ///
    /// Contém o quadro recebido mais os que estavam esperando por ele.
    Ready(Vec<Frame>),
    /// A fila de reordenação estourou. Derrube o enlace.
    ///
    /// Prosseguir significaria entregar com lacuna, e uma lacuna no canal de teclado é uma
    /// tecla presa.
    Overflow,
}

/// O lado receptor de um canal confiável.
#[derive(Debug, Clone, Default)]
pub struct Receiver {
    /// A confirmação corrente: o que já **chegou**, em ordem ou não.
    ///
    /// Confirmar o que chegou, e não o que foi entregue, é o certo: o emissor precisa parar
    /// de retransmitir um quadro que está aqui esperando o anterior.
    ack: Option<Ack>,
    /// A próxima sequência a **entregar**.
    next: Option<Sequence>,
    /// Os quadros que chegaram adiantados, ordenados pela sequência.
    held: BTreeMap<u32, Frame>,
}

impl Receiver {
    /// Um receptor que ainda não viu nada.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Registra um quadro e diz o que fazer com ele.
    pub fn accept(&mut self, seq: Sequence, frame: Frame) -> Delivery {
        if self.is_known(seq) {
            return Delivery::Duplicate;
        }
        // Anterior ao que já foi entregue: chegou tarde demais para importar. Guardá-lo na
        // fila seria esperar para sempre por um buraco que já passou.
        if self.next.is_some_and(|next| next.is_newer_than(seq)) {
            return Delivery::Duplicate;
        }
        self.record(seq);

        let expected = match self.next {
            // O primeiro quadro define o ponto de partida: não há como saber o que veio antes,
            // e esperar por sequências que nunca existiram travaria a sessão para sempre.
            None => seq,
            Some(expected) => expected,
        };

        if seq != expected {
            if self.held.len() >= REORDER_LIMIT {
                return Delivery::Overflow;
            }
            self.held.insert(seq.get(), frame);
            return Delivery::Buffered;
        }

        let mut ready = vec![frame];
        let mut cursor = expected.next();
        while let Some(waiting) = self.held.remove(&cursor.get()) {
            ready.push(waiting);
            cursor = cursor.next();
        }
        self.next = Some(cursor);
        Delivery::Ready(ready)
    }

    /// Se esta sequência já chegou antes.
    fn is_known(&self, seq: Sequence) -> bool {
        if self.held.contains_key(&seq.get()) {
            return true;
        }
        self.ack.is_some_and(|ack| ack.covers(seq))
    }

    /// Marca a sequência como chegada, para a confirmação.
    fn record(&mut self, seq: Sequence) {
        let Some(current) = self.ack else {
            self.ack = Some(Ack::new(seq));
            return;
        };

        if seq.is_newer_than(current.cumulative) {
            // Avança a janela. O bitmap desloca junto, e o que sair dela é esquecido — é o
            // preço de não guardar histórico ilimitado.
            // `seq` menos `cumulative`, não o contrário: é quantas posições a janela avança.
            // Invertido, o deslocamento vira um número enorme, o bitmap é zerado a cada
            // avanço, e nada além da própria cumulativa parece confirmado — o emissor
            // retransmite tudo até desistir.
            let shift = seq.distance_from(current.cumulative);
            let mut bits = if shift >= 32 {
                0
            } else {
                current.bits << shift
            };
            if (1..=32).contains(&shift) {
                // A que era cumulativa passa a ser uma das anteriores.
                bits |= 1u32 << (shift - 1);
            }
            self.ack = Some(Ack {
                cumulative: seq,
                bits,
            });
        } else {
            self.ack = Some(current.with(seq));
        }
    }

    /// A confirmação a enviar, se já houver o que confirmar.
    #[must_use]
    pub const fn ack_to_send(&self) -> Option<Ack> {
        self.ack
    }

    /// Quantos quadros estão esperando o buraco à frente ser preenchido.
    #[must_use]
    pub fn buffered(&self) -> usize {
        self.held.len()
    }

    /// Esvazia. Chamado a cada handshake novo.
    pub fn reset(&mut self) {
        self.ack = None;
        self.next = None;
        self.held.clear();
    }
}
