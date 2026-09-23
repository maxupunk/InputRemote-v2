//! Canal 4: o texto do clipboard, conduzido pela sessão.
//!
//! O que cada ponta guarda — a oferta, os pedaços, a montagem — mora em `ir-area`. Aqui fica só o
//! que depende da sessão: mandar pela rota, no ritmo da janela de confirmação. Um pedaço mais de 32
//! sequências à frente do mais antigo sem confirmação deixa esse mais antigo fora do alcance, e o
//! enlace cai por uma mensagem que chegou; por isso sai um punhado por batida, só dentro do alcance.

use ir_proto::channel::ChannelId;
use ir_proto::message::{ClipboardMessage, Message};

use super::Session;
use crate::event::{ClipText, Command, CommandBatch};
use crate::time::Timestamp;

/// Quantos pedaços por batida, sobre datagrama.
const POR_BATIDA_DATAGRAMA: usize = 2;

impl Session {
    /// Oferece ao par o texto que o usuário copiou.
    pub(super) fn on_clipboard_text(
        &mut self,
        now: Timestamp,
        texto: ClipText,
        out: &mut CommandBatch,
    ) {
        if !self.phase.is_established() {
            return;
        }
        let oferta = self.area.oferecer(texto);
        self.send(now, Message::Clipboard(oferta), out);
    }

    /// Uma mensagem do canal 4 chegou.
    pub(super) fn on_clipboard_message(
        &mut self,
        now: Timestamp,
        mensagem: ClipboardMessage,
        out: &mut CommandBatch,
    ) {
        let resposta = match mensagem {
            ClipboardMessage::Offer {
                id,
                kind,
                size,
                hash,
            } => Some(self.area.ofereceram(id, kind, size, hash)),
            ClipboardMessage::Request { id } => {
                self.area.pedido(id);
                self.pump_clipboard(now, out);
                None
            }
            ClipboardMessage::Chunk { id, index, data } => {
                self.area.pedaco(id, index, &data);
                None
            }
            ClipboardMessage::Done { id } => {
                if let Some(texto) = self.area.fim(id) {
                    out.push(Command::ClipboardText(texto));
                }
                None
            }
            ClipboardMessage::Decline { id, .. } => {
                self.area.recusaram(id);
                None
            }
            // Variante de versão futura: não há o que fazer com ela.
            _ => None,
        };
        if let Some(resposta) = resposta {
            self.send(now, Message::Clipboard(resposta), out);
        }
    }

    /// Manda os próximos pedaços, se houver vez.
    pub(super) fn pump_clipboard(&mut self, now: Timestamp, out: &mut CommandBatch) {
        if self.route.is_none() || !self.phase.is_established() {
            return;
        }
        // Toda rota é tratada como datagrama (`route`): o ritmo é o da janela, em qualquer portador.
        for _ in 0..POR_BATIDA_DATAGRAMA {
            // Sobre stream não há janela, e a resposta é sempre sim.
            let proxima = self.seqs.peek(ChannelId::ClipboardText);
            if !self
                .reliability
                .within_ack_reach(ChannelId::ClipboardText, proxima)
            {
                return;
            }
            let Some(pedaco) = self.area.proximo_pedaco() else {
                return;
            };
            self.send(now, Message::Clipboard(pedaco), out);
        }
    }
}
