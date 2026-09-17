//! O enquadramento do canal de dados. Parte pura, sem socket.
//!
//! TCP entrega um *stream*: bytes em ordem, sem perda, picados como a rede quiser. Não existe
//! "um pacote, uma mensagem". [03, §2](../../../docs/03-protocolo.md) manda um **prefixo de
//! tamanho `u32`** antes de cada corpo, e é ele que devolve a fronteira que o meio não dá.
//!
//! # O contador não viaja
//!
//! Como no RFCOMM, e pelo mesmo motivo: o meio garante ordem, então quem recebe **conta** os
//! quadros — o primeiro é 1, o seguinte é 2 — e chega ao mesmo número que o
//! [`Transport`](ir_crypto::Transport) emitiu do outro lado. No UDP o contador precisa viajar
//! em claro porque a ordem não é garantida; aqui seriam 8 bytes gastos para transportar o que o
//! receptor já sabe.
//!
//! O preço é o mesmo: um quadro que não abre significa contagens divergentes, e o enlace **cai**
//! em vez de ignorar o quadro. Divergir em silêncio não é um resultado possível.
//!
//! # Sem byte de modo, sem byte de espécie
//!
//! O canal de dados **nunca pareia**. Ele usa a identidade que a sessão já fixou
//! ([01, §3.3](../../../docs/01-visao-e-escopo.md)), logo o padrão Noise é sempre `IK` e o
//! respondedor não precisa de um byte em claro para escolher — não há escolha. E como não há
//! confirmação nem recusa de pareamento a transportar, o texto claro é o quadro do `ir-proto` e
//! mais nada.
//!
//! É a diferença real em relação ao `ir-bt`, que precisa dos dois bytes porque é lá que o
//! primeiro encontro acontece.
//!
//! # Uma observação sobre a largura do prefixo
//!
//! Com o teto do texto claro corrigido para 65 519 B
//! ([ADR-0010](../../../docs/adr/0010-canal-de-dados-em-tcp-proprio.md)), o maior corpo
//! possível é exatamente 65 535 — que caberia num `u16`. O prefixo continua `u32` porque é o
//! que a especificação diz, e trocar formato de fio para economizar dois bytes a cada 64 KiB
//! seria churn sem ganho mensurável.

use ir_proto::limits;

use crate::error::{NetError, Result};

/// Bytes do prefixo de tamanho.
pub const PREFIX: usize = 4;

/// O que o Noise acrescenta a cada quadro cifrado: a tag Poly1305 de 16 bytes.
const TAG: usize = 16;

/// O maior corpo que pode vir depois do prefixo.
///
/// O teto de texto claro do portador, mais a tag. É contra este número que se confere **antes**
/// de alocar qualquer coisa ([04, §1](../../../docs/04-seguranca.md)).
pub const MAX_BODY: usize = limits::MAX_TCP_PLAINTEXT + TAG;

/// Põe o prefixo de tamanho num corpo, deixando-o pronto para o socket.
///
/// # Errors
///
/// [`NetError::TooLarge`] se o corpo passa de [`MAX_BODY`]. Falhar aqui é falha nossa, e custa
/// menos que descobrir do outro lado.
pub fn frame(body: &[u8]) -> Result<Vec<u8>> {
    if body.len() > MAX_BODY {
        return Err(NetError::TooLarge {
            size: body.len(),
            limit: MAX_BODY,
        });
    }
    let Ok(size) = u32::try_from(body.len()) else {
        return Err(NetError::TooLarge {
            size: body.len(),
            limit: MAX_BODY,
        });
    };
    let mut out = Vec::with_capacity(PREFIX + body.len());
    out.extend_from_slice(&size.to_le_bytes());
    out.extend_from_slice(body);
    Ok(out)
}

/// Junta os pedaços que chegam do *stream* e devolve um corpo completo de cada vez.
///
/// É o tipo que sabe que o TCP não respeita fronteira de mensagem. Alimente com o que o socket
/// entregou, do tamanho que vier, e peça corpos até não haver mais nenhum inteiro.
#[derive(Debug, Default)]
pub struct Framer {
    pending: Vec<u8>,
}

impl Framer {
    /// Um desenquadrador vazio.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            pending: Vec::new(),
        }
    }

    /// Guarda os bytes que acabaram de chegar do socket.
    pub fn feed(&mut self, bytes: &[u8]) {
        self.pending.extend_from_slice(bytes);
    }

    /// Quantos bytes ainda não formaram um corpo inteiro.
    #[must_use]
    pub fn pending(&self) -> usize {
        self.pending.len()
    }

    /// O próximo corpo completo, se já chegou inteiro.
    ///
    /// # Errors
    ///
    /// [`NetError::TooLarge`] se o tamanho anunciado passa de [`MAX_BODY`] — conferido antes de
    /// reservar memória. Um anúncio absurdo derruba o enlace, e não a máquina.
    pub fn next_body(&mut self) -> Result<Option<Vec<u8>>> {
        let Some(prefix) = self.pending.get(..PREFIX) else {
            return Ok(None); // nem o tamanho chegou ainda
        };
        let Ok(bytes) = <[u8; PREFIX]>::try_from(prefix) else {
            return Ok(None);
        };
        let size = usize::try_from(u32::from_le_bytes(bytes)).unwrap_or(usize::MAX);
        if size > MAX_BODY {
            return Err(NetError::TooLarge {
                size,
                limit: MAX_BODY,
            });
        }
        let end = PREFIX + size;
        if self.pending.len() < end {
            return Ok(None); // o corpo ainda está chegando
        }
        let body: Vec<u8> = self.pending.drain(..end).skip(PREFIX).collect();
        Ok(Some(body))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_framed_body_comes_back_whole() {
        let body = b"um bloco de arquivo";
        let mut framer = Framer::new();
        framer.feed(&frame(body).unwrap());
        assert_eq!(
            framer.next_body().unwrap().as_deref(),
            Some(body.as_slice())
        );
        assert_eq!(framer.next_body().unwrap(), None);
    }

    #[test]
    fn a_body_split_byte_by_byte_still_comes_back_whole() {
        // O caso que o prefixo existe para resolver, no pior formato possível.
        let body = vec![0x5a; 4096];
        let on_wire = frame(&body).unwrap();
        let mut framer = Framer::new();
        for byte in &on_wire {
            assert_eq!(
                framer.next_body().unwrap(),
                None,
                "não pode entregar pela metade"
            );
            framer.feed(&[*byte]);
        }
        assert_eq!(framer.next_body().unwrap(), Some(body));
    }

    #[test]
    fn several_bodies_glued_together_come_back_one_by_one() {
        let mut glued = Vec::new();
        for n in 1u8..=4 {
            glued.extend_from_slice(&frame(&vec![n; usize::from(n) * 100]).unwrap());
        }
        let mut framer = Framer::new();
        framer.feed(&glued);
        for n in 1u8..=4 {
            let body = framer.next_body().unwrap().expect("corpo inteiro");
            assert_eq!(body, vec![n; usize::from(n) * 100]);
        }
        assert_eq!(framer.next_body().unwrap(), None);
        assert_eq!(framer.pending(), 0);
    }

    #[test]
    fn an_empty_body_is_a_body() {
        // Não é um caso útil, mas é um caso possível — e um desenquadrador que travasse aqui
        // pararia o enlace em vez de seguir.
        let mut framer = Framer::new();
        framer.feed(&frame(&[]).unwrap());
        assert_eq!(framer.next_body().unwrap(), Some(Vec::new()));
    }

    #[test]
    fn the_largest_possible_body_is_accepted() {
        let body = vec![0u8; MAX_BODY];
        let on_wire = frame(&body).unwrap();
        assert_eq!(on_wire.len(), PREFIX + MAX_BODY);
        let mut framer = Framer::new();
        framer.feed(&on_wire);
        assert_eq!(framer.next_body().unwrap(), Some(body));
    }

    #[test]
    fn a_body_one_byte_over_the_cap_is_refused_when_framing() {
        let err = frame(&vec![0u8; MAX_BODY + 1]).unwrap_err();
        assert!(matches!(err, NetError::TooLarge { limit, .. } if limit == MAX_BODY));
    }

    #[test]
    fn a_lying_prefix_is_refused_before_any_allocation() {
        // A defesa de docs/04 §1: quatro bytes de prefixo podem pedir 4 GiB. A recusa acontece
        // olhando o número, sem reservar nada — e o corpo anunciado nunca chega.
        let mut framer = Framer::new();
        framer.feed(&u32::MAX.to_le_bytes());
        let err = framer.next_body().unwrap_err();
        assert!(matches!(
            err,
            NetError::TooLarge {
                size,
                limit
            } if size == usize::try_from(u32::MAX).unwrap_or(usize::MAX) && limit == MAX_BODY
        ));
    }

    #[test]
    fn a_prefix_one_over_the_cap_is_refused_too() {
        let mut framer = Framer::new();
        let size = u32::try_from(MAX_BODY + 1).expect("cabe em u32");
        framer.feed(&size.to_le_bytes());
        assert!(framer.next_body().is_err());
    }

    #[test]
    fn the_cap_is_exactly_the_plaintext_ceiling_plus_the_noise_tag() {
        // Amarra o teto do enquadramento ao teto do Noise. Se `MAX_TCP_PLAINTEXT` mudar sem
        // este número mudar junto, o quadro cheio deixaria de caber e o sintoma apareceria na
        // bancada, não aqui.
        assert_eq!(MAX_BODY, limits::MAX_TCP_PLAINTEXT + 16);
        assert!(u32::try_from(MAX_BODY).is_ok());
    }
}
