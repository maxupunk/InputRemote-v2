//! A manutenção da confiabilidade: reenviar o que venceu e confirmar o que chegou.
//!
//! Vale para qualquer rota. Desde a versão 2 do protocolo a sessão trata todo portador de entrada
//! como datagrama ([`route`](super::route)), então reenvio e confirmação pura rodam também sobre
//! RFCOMM — num meio que não perde, as janelas só esvaziam a cada confirmação, e o custo é uma
//! confirmação de poucos bytes a cada 20 ms enquanto houver algo pendente.

use ir_proto::frame::{Frame, Sequence};
use ir_proto::message::{Control, Message};

use crate::event::{CommandBatch, LinkDown};
use crate::reliability::Due;
use crate::session::Session;
use crate::time::{Millis, Timestamp};

/// Intervalo entre confirmações puras.
///
/// Origem: `docs/03-protocolo.md` §4.1 — a cada 20 ms enquanto houver algo pendente.
const ACK_INTERVAL: Millis = Millis(20);

impl Session {
    /// Reenvia o que venceu, ou derruba o enlace se alguma mensagem esgotou as tentativas.
    ///
    /// Vale para qualquer rota: desde a versão 2 a sessão trata todo portador como datagrama, e
    /// sobre um meio confiável as janelas simplesmente esvaziam a cada confirmação (`route`).
    pub(super) fn service_retransmissions(&mut self, now: Timestamp, out: &mut CommandBatch) {
        if self.route.is_none() {
            return;
        }

        let timings = self.config.timings;
        let due = self
            .reliability
            .on_tick(now, timings.min_retransmit, timings.link_timeout);

        match due {
            Due::Idle => {}
            Due::Retransmit(frames) => {
                for frame in frames {
                    self.dispatch_on_route(frame, out);
                }
            }
            Due::GiveUp { .. } => {
                // Passado o prazo sem confirmação, prosseguir seguiria com uma lacuna no canal de
                // teclado. Se a mensagem perdida for um `KeyUp`, a tecla fica presa na
                // máquina do outro — e o usuário não sabe o que aconteceu nem como sair.
                self.tear_down(now, LinkDown::Timeout, out);
            }
        }
    }

    /// Manda uma confirmação pura quando há o que confirmar e nada saindo para carregá-la.
    ///
    /// É o caso da digitação contínua: o servidor manda tecla após tecla e o cliente não tem
    /// nada a dizer. Sem isto, a janela do servidor encheria depois de 64 teclas e a sessão
    /// cairia no meio de uma frase.
    pub(super) fn send_bare_ack_if_needed(&mut self, now: Timestamp, out: &mut CommandBatch) {
        if self.route.is_none() {
            return;
        }
        if !now.elapsed_at_least(self.clock.last_bare_ack, ACK_INTERVAL) {
            return;
        }

        // Uma confirmação por canal com algo a confirmar. Um quadro carrega a confirmação de
        // **um** canal, e mandar só a do canal mais urgente deixaria os outros sem
        // confirmação nenhuma — a janela deles encheria e a sessão cairia por um caminho que
        // ninguém associaria à causa.
        let mut sent_any = false;
        for channel in ir_proto::channel::ChannelId::ALL {
            let Some(ack) = self.reliability.ack_for(channel) else {
                continue;
            };
            // Sequência zero e nunca contada: uma confirmação pura não faz parte do fluxo
            // ordenado. Se ela consumisse número de sequência sem ser retransmitida, perder
            // uma criaria um buraco que nunca seria preenchido, e tudo depois dela ficaria
            // esperando para sempre.
            let frame = Frame::new(Message::Control(Control::AckOnly), Sequence::ZERO)
                .with_ack(channel, ack)
                .in_epoch(self.incarnations.local());
            self.dispatch_on_route(frame, out);
            sent_any = true;
        }
        if sent_any {
            self.clock.last_bare_ack = now;
        }
    }
}
