//! Canal 4: o texto do clipboard, oferecido, pedido, em pedaços e conferido.
//!
//! ```text
//! A: Offer{id, tamanho, resumo}  ─►
//!                                ◄─  Request{id}          B aceita, ou Decline com motivo
//! A: Chunk{id, 0..n} … Done{id}  ─►  B confere tamanho e BLAKE3 → Command::ClipboardText
//! ```
//!
//! # A cadência
//!
//! Os pedaços não saem todos de uma vez. Sobre datagrama, um pedaço mais de 32 sequências à frente
//! do mais antigo sem confirmação deixa esse mais antigo fora do alcance da confirmação, e o enlace
//! cai por uma mensagem que chegou ([`Sender::within_ack_reach`](crate::reliability::Sender)); com
//! a janela cheia, cai também ([`Session::send`]). Sobre stream, o rádio é o mesmo que carrega o
//! `KeyUp`. Então sai um pedaço por batida (dois sobre datagrama, e só dentro do alcance): 256 KiB
//! atravessam em poucos segundos, e o teclado nunca espera atrás deles.
//!
//! # O tamanho do pedaço
//!
//! O do menor portador, sempre. Um pedaço montado para UDP e enviado depois de trocar para
//! Bluetooth não caberia no quadro; o mesmo tamanho em todo portador tira esse caso de existir.

use std::collections::VecDeque;

use ir_proto::channel::ChannelId;
use ir_proto::limits::{MAX_CLIPBOARD_TEXT_OFF_TCP, MAX_RFCOMM_PLAINTEXT};
use ir_proto::message::{ClipId, ClipKind, ClipboardMessage, DeclineReason, Message};

use super::Session;
use crate::event::{ClipText, Command, CommandBatch};
use crate::time::Timestamp;

/// Bytes de texto por pedaço. O resto do quadro — canal, sequência, época, confirmação e os
/// campos do `Chunk` — cabe com folga nos 64 de diferença; há teste que codifica o pior caso.
pub(super) const PEDACO: usize = MAX_RFCOMM_PLAINTEXT - 64;

/// Quantos pedaços por batida, sobre datagrama.
const POR_BATIDA_DATAGRAMA: usize = 2;

/// O estado do canal 4, nos dois sentidos.
#[derive(Clone, Default)]
pub(crate) struct Area {
    /// O próximo identificador de oferta.
    proximo: u32,
    /// O que oferecemos e o par ainda não pediu.
    oferta: Option<(ClipId, ClipText)>,
    /// Pedaços prontos, esperando a vez.
    fila: VecDeque<ClipboardMessage>,
    /// O que o par está mandando.
    montagem: Option<Montagem>,
}

/// Um texto chegando.
#[derive(Clone)]
struct Montagem {
    id: ClipId,
    tamanho: usize,
    resumo: [u8; 32],
    proximo: u32,
    dados: Vec<u8>,
}

impl std::fmt::Debug for Area {
    /// Contagens, nunca bytes: a fila e a montagem **são** o texto copiado.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Area")
            .field("oferecendo", &self.oferta.is_some())
            .field("na_fila", &self.fila.len())
            .field("recebendo", &self.montagem.as_ref().map(|m| m.dados.len()))
            .finish_non_exhaustive()
    }
}

impl Area {
    /// Esquece tudo. Uma sessão nova não continua texto de outra.
    pub(super) fn reset(&mut self) {
        self.oferta = None;
        self.fila.clear();
        self.montagem = None;
    }

    /// Guarda o texto e devolve a oferta a mandar. A oferta anterior, e o que dela faltava
    /// sair, perdem a validade: o usuário copiou outra coisa.
    fn oferecer(&mut self, texto: ClipText) -> ClipboardMessage {
        self.proximo = self.proximo.wrapping_add(1);
        let id = ClipId(self.proximo);
        let bytes = texto.as_str().as_bytes();
        let oferta = ClipboardMessage::Offer {
            id,
            kind: ClipKind::Text,
            // `ClipText` não passa de 256 KiB: cabe em `u32` por construção.
            size: u32::try_from(bytes.len()).unwrap_or(u32::MAX),
            hash: *blake3::hash(bytes).as_bytes(),
        };
        self.fila.clear();
        self.oferta = Some((id, texto));
        oferta
    }

    /// O par pediu: a oferta vira pedaços na fila.
    fn pedido(&mut self, id: ClipId) {
        let Some((oferecida, texto)) = self.oferta.take_if(|(oferecida, _)| *oferecida == id)
        else {
            return; // de uma oferta já substituída
        };
        let mut index = 0u32;
        for data in texto.as_str().as_bytes().chunks(PEDACO) {
            self.fila.push_back(ClipboardMessage::Chunk {
                id: oferecida,
                index,
                data: data.to_vec(),
            });
            index = index.wrapping_add(1);
        }
        self.fila
            .push_back(ClipboardMessage::Done { id: oferecida });
    }

    /// O par ofereceu. Devolve a resposta: o pedido, ou a recusa com o motivo.
    fn ofereceram(
        &mut self,
        id: ClipId,
        kind: ClipKind,
        size: u32,
        hash: [u8; 32],
    ) -> ClipboardMessage {
        let tamanho = usize::try_from(size).unwrap_or(usize::MAX);
        let recusa = if kind == ClipKind::Text {
            (tamanho > MAX_CLIPBOARD_TEXT_OFF_TCP).then_some(DeclineReason::TooLargeForCarrier)
        } else {
            Some(DeclineReason::KindNotSupported)
        };
        if let Some(reason) = recusa {
            return ClipboardMessage::Decline { id, reason };
        }
        // Reservar o tamanho anunciado é seguro: ele acabou de ser limitado a 256 KiB.
        self.montagem = Some(Montagem {
            id,
            tamanho,
            resumo: hash,
            proximo: 0,
            dados: Vec::with_capacity(tamanho),
        });
        ClipboardMessage::Request { id }
    }

    /// Um pedaço chegou. Fora de ordem ou além do anunciado, a montagem inteira é abandonada.
    fn pedaco(&mut self, id: ClipId, index: u32, data: &[u8]) {
        let Some(montagem) = self.montagem.as_mut().filter(|m| m.id == id) else {
            return; // de uma oferta anterior, ainda em trânsito
        };
        if index != montagem.proximo || montagem.dados.len() + data.len() > montagem.tamanho {
            self.montagem = None;
            return;
        }
        montagem.dados.extend_from_slice(data);
        montagem.proximo = montagem.proximo.wrapping_add(1);
    }

    /// Terminou: o texto, se chegou inteiro e é o que foi anunciado.
    fn fim(&mut self, id: ClipId) -> Option<ClipText> {
        let montagem = self.montagem.take_if(|m| m.id == id)?;
        let inteiro = montagem.dados.len() == montagem.tamanho
            && *blake3::hash(&montagem.dados).as_bytes() == montagem.resumo;
        if !inteiro {
            return None;
        }
        ClipText::new(String::from_utf8(montagem.dados).ok()?)
    }
}

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
                self.area.oferta.take_if(|(oferecida, _)| *oferecida == id);
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
            let Some(pedaco) = self.area.fila.pop_front() else {
                return;
            };
            self.send(now, Message::Clipboard(pedaco), out);
        }
    }
}

#[cfg(test)]
mod testes;
