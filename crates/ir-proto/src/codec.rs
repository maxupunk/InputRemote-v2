//! Codificação e decodificação de quadros.
//!
//! Este é o código que recebe bytes de um rádio aberto dentro de um processo `SYSTEM`, antes
//! de haver qualquer usuário logado (`docs/04-seguranca.md` §1). Ele é `forbid(unsafe_code)`
//! por herança do crate, não indexa fatia sem verificar, e é o alvo obrigatório do fuzzing
//! de `docs/10-testes-e-validacao.md` §3.
//!
//! Três decisões de rigor, todas deliberadas:
//!
//! 1. **Byte sobrando é erro.** Não se ignora cauda. Duas pontas que discordam do formato
//!    precisam parar, não continuar adivinhando.
//! 2. **Canal incompatível com o portador é erro**, nas duas direções — ao codificar e ao
//!    decodificar. Codificar também verifica porque é onde o defeito é nosso, e falhar cedo
//!    é mais barato que descobrir do outro lado.
//! 3. **Tamanho é conferido contra o portador**, não contra um número global.

use crate::carrier::Carrier;
use crate::error::{ProtoError, Result};
use crate::frame::Frame;

/// Codifica um quadro para o portador dado, alocando.
///
/// Fora do caminho quente. No caminho quente use [`encode_into`], que não aloca.
///
/// # Errors
///
/// - [`ProtoError::WrongChannel`] se o canal do quadro não pode viajar por este portador.
/// - [`ProtoError::TooLarge`] se o resultado passa do limite do portador.
/// - [`ProtoError::Malformed`] se a serialização falhar.
pub fn encode(frame: &Frame, carrier: Carrier) -> Result<Vec<u8>> {
    check_channel(frame, carrier)?;
    let bytes = postcard::to_allocvec(frame)?;
    check_len(bytes.len(), carrier)?;
    Ok(bytes)
}

/// Codifica um quadro num buffer já existente, sem alocar.
///
/// É a forma usada no caminho quente, com buffer reaproveitado — regra 2 de
/// `docs/02-arquitetura.md` §6.
///
/// # Errors
///
/// Os mesmos de [`encode`], mais [`ProtoError::TooLarge`] quando o buffer não cabe o quadro.
pub fn encode_into<'buf>(
    frame: &Frame,
    buf: &'buf mut [u8],
    carrier: Carrier,
) -> Result<&'buf [u8]> {
    check_channel(frame, carrier)?;
    let limit = carrier.max_plaintext().min(buf.len());
    let written = postcard::to_slice(frame, buf).map_err(|err| match err {
        postcard::Error::SerializeBufferFull => ProtoError::TooLarge {
            actual: limit + 1,
            limit,
        },
        other => ProtoError::from(other),
    })?;
    check_len(written.len(), carrier)?;
    Ok(written)
}

/// Decodifica um quadro recebido por este portador.
///
/// # Errors
///
/// - [`ProtoError::TooLarge`] se a entrada já passa do limite do portador — conferido
///   **antes** de decodificar, porque é a verificação mais barata.
/// - [`ProtoError::Truncated`] se os bytes acabam no meio.
/// - [`ProtoError::TrailingBytes`] se sobra byte depois do fim.
/// - [`ProtoError::WrongChannel`] se o canal decodificado não pode usar este portador.
/// - [`ProtoError::Malformed`] para qualquer outra inconsistência.
pub fn decode(bytes: &[u8], carrier: Carrier) -> Result<Frame> {
    check_len(bytes.len(), carrier)?;
    let (frame, rest) = postcard::take_from_bytes::<Frame>(bytes)?;
    if !rest.is_empty() {
        return Err(ProtoError::TrailingBytes);
    }
    check_channel(&frame, carrier)?;
    Ok(frame)
}

fn check_channel(frame: &Frame, carrier: Carrier) -> Result<()> {
    let channel = frame.channel();
    if channel.allows(carrier) {
        Ok(())
    } else {
        Err(ProtoError::WrongChannel {
            channel: channel.name(),
        })
    }
}

fn check_len(len: usize, carrier: Carrier) -> Result<()> {
    let limit = carrier.max_plaintext();
    if len > limit {
        Err(ProtoError::TooLarge { actual: len, limit })
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frame::{Ack, Sequence};
    use crate::input::{Button, HidUsage, Modifiers, PointerDelta};
    use crate::limits;
    use crate::message::{BulkMessage, Control, InputMessage, Message, PointerMessage, TransferId};

    fn key_frame() -> Frame {
        Frame::new(
            Message::Input(InputMessage::KeyDown {
                usage: HidUsage(0x04),
                mods: Modifiers::LEFT_SHIFT,
            }),
            Sequence(7),
        )
    }

    #[test]
    fn round_trip_over_every_allowed_carrier() {
        let frame = key_frame();
        for carrier in [Carrier::Rfcomm, Carrier::Udp] {
            let bytes = encode(&frame, carrier).unwrap();
            assert_eq!(decode(&bytes, carrier).unwrap(), frame);
        }
    }

    #[test]
    fn round_trip_preserves_a_carried_ack() {
        let frame = key_frame().with_ack(
            crate::channel::ChannelId::ReliableInput,
            Ack::new(Sequence(42)).with(Sequence(41)),
        );
        let bytes = encode(&frame, Carrier::Udp).unwrap();
        assert_eq!(decode(&bytes, Carrier::Udp).unwrap(), frame);
    }

    #[test]
    fn input_is_refused_on_tcp_when_encoding_and_decoding() {
        let frame = key_frame();
        let err = encode(&frame, Carrier::Tcp).unwrap_err();
        assert_eq!(
            err,
            ProtoError::WrongChannel {
                channel: frame.channel().name()
            }
        );

        // E também na decodificação: bytes válidos chegando pelo portador errado.
        let bytes = encode(&frame, Carrier::Udp).unwrap();
        assert_eq!(
            decode(&bytes, Carrier::Tcp).unwrap_err(),
            ProtoError::WrongChannel {
                channel: frame.channel().name()
            }
        );
    }

    #[test]
    fn a_full_file_block_fits_a_tcp_frame() {
        // A razão de `MAX_FILE_BLOCK` existir separado de `MAX_TCP_PLAINTEXT`: o bloco cheio
        // ainda tem de caber **com** o cabeçalho da própria mensagem — canal, discriminante,
        // identificador, índice do item e deslocamento.
        //
        // Conferido codificando o pior caso, e não pela aritmética: quanto o `postcard` gasta
        // no prefixo de uma fatia de 60 KiB e nos varints é detalhe dele, não nosso. Se um dia
        // essa conta mudar, é aqui que aparece — não na bancada, no primeiro bloco cheio.
        let frame = Frame::new(
            Message::Bulk(BulkMessage::FileBlock {
                id: TransferId(u32::MAX),
                item: u32::MAX,
                offset: u64::MAX,
                data: vec![0xa5; limits::MAX_FILE_BLOCK],
            }),
            Sequence(u32::MAX),
        );
        let bytes = encode(&frame, Carrier::Tcp).expect("o bloco cheio tem de caber");
        assert_eq!(decode(&bytes, Carrier::Tcp).unwrap(), frame);

        let folga = limits::MAX_TCP_PLAINTEXT - bytes.len();
        assert!(
            folga >= 64,
            "a folga do cabeçalho caiu para {folga} B — apertado demais para ser seguro"
        );
    }

    #[test]
    fn a_file_block_one_byte_over_the_limit_is_still_encodable() {
        // O teto do bloco é nossa política, não o teto do portador: passar um byte dele não
        // é erro de codec. Este teste existe para que ninguém "corrija" `MAX_FILE_BLOCK`
        // achando que ele é o limite físico — quem o aplica é quem monta o bloco.
        let frame = Frame::new(
            Message::Bulk(BulkMessage::FileBlock {
                id: TransferId(1),
                item: 0,
                offset: 0,
                data: vec![0u8; limits::MAX_FILE_BLOCK + 1],
            }),
            Sequence::ZERO,
        );
        assert!(encode(&frame, Carrier::Tcp).is_ok());
    }

    #[test]
    fn a_frame_past_the_noise_ceiling_is_refused_instead_of_failing_later() {
        // Acima de `MAX_TCP_PLAINTEXT` não há o que negociar: o Noise não cifraria. O codec
        // recusa aqui, onde o defeito é nosso e barato, em vez de o erro aparecer como
        // "enlace caiu" do outro lado.
        let frame = Frame::new(
            Message::Bulk(BulkMessage::FileBlock {
                id: TransferId(1),
                item: 0,
                offset: 0,
                data: vec![0u8; limits::MAX_TCP_PLAINTEXT],
            }),
            Sequence::ZERO,
        );
        assert!(matches!(
            encode(&frame, Carrier::Tcp),
            Err(ProtoError::TooLarge { .. })
        ));
    }

    #[test]
    fn bulk_is_refused_outside_tcp() {
        let frame = Frame::new(
            Message::Bulk(BulkMessage::Accept { id: TransferId(1) }),
            Sequence::ZERO,
        );
        assert!(encode(&frame, Carrier::Tcp).is_ok());
        for carrier in [Carrier::Rfcomm, Carrier::Udp] {
            assert!(
                encode(&frame, carrier).is_err(),
                "{carrier} não deveria aceitar dados"
            );
        }
    }

    #[test]
    fn trailing_bytes_are_an_error_not_ignored() {
        let mut bytes = encode(&key_frame(), Carrier::Udp).unwrap();
        bytes.push(0x00);
        assert_eq!(
            decode(&bytes, Carrier::Udp).unwrap_err(),
            ProtoError::TrailingBytes
        );
    }

    #[test]
    fn truncation_at_every_length_is_an_error_and_never_panics() {
        let bytes = encode(&key_frame(), Carrier::Udp).unwrap();
        for cut in 0..bytes.len() {
            let err = decode(&bytes[..cut], Carrier::Udp).unwrap_err();
            assert!(
                matches!(err, ProtoError::Truncated | ProtoError::Malformed),
                "corte em {cut} deu {err:?}"
            );
        }
    }

    #[test]
    fn arbitrary_garbage_never_panics() {
        // Varredura pequena, determinística. O fuzzing de verdade está em fuzz/, mas este
        // teste guarda o caso mais óbvio no CI de todo commit.
        for seed in 0u16..=2000 {
            let [low, high] = seed.to_le_bytes();
            let [mixed, _] = seed.wrapping_mul(31).to_le_bytes();
            let [shifted, _] = seed.wrapping_add(7).to_le_bytes();
            let bytes = [low, high, mixed, shifted];
            let _ = decode(&bytes, Carrier::Udp);
            let _ = decode(&bytes, Carrier::Tcp);
            let _ = decode(&bytes, Carrier::Rfcomm);
        }
    }

    #[test]
    fn oversized_input_is_refused_before_decoding() {
        let too_big = vec![0u8; limits::MAX_RFCOMM_PLAINTEXT + 1];
        let err = decode(&too_big, Carrier::Rfcomm).unwrap_err();
        assert_eq!(
            err,
            ProtoError::TooLarge {
                actual: limits::MAX_RFCOMM_PLAINTEXT + 1,
                limit: limits::MAX_RFCOMM_PLAINTEXT,
            }
        );
    }

    #[test]
    fn encode_into_matches_encode_and_does_not_allocate() {
        let frame = key_frame();
        let mut buf = [0u8; limits::MAX_UDP_PLAINTEXT];
        let written = encode_into(&frame, &mut buf, Carrier::Udp).unwrap();
        assert_eq!(written, encode(&frame, Carrier::Udp).unwrap().as_slice());
    }

    #[test]
    fn encode_into_reports_a_buffer_that_is_too_small() {
        let mut tiny = [0u8; 2];
        let err = encode_into(&key_frame(), &mut tiny, Carrier::Udp).unwrap_err();
        assert!(matches!(err, ProtoError::TooLarge { .. }));
    }

    #[test]
    fn hot_path_messages_stay_within_the_budget() {
        // O teto de docs/03-protocolo.md §9. Se uma mensagem de entrada passar disto, alguma
        // coisa entrou no caminho quente que não deveria.
        let mods = Modifiers::LEFT_CTRL.union(Modifiers::RIGHT_ALT);
        let hot = [
            Message::Input(InputMessage::KeyDown {
                usage: HidUsage::MAX,
                mods,
            }),
            Message::Input(InputMessage::KeyUp {
                usage: HidUsage::MAX,
                mods,
            }),
            Message::Input(InputMessage::ButtonDown {
                button: Button::Forward,
                mods,
            }),
            Message::Input(InputMessage::ReleaseAll),
            Message::Pointer(PointerMessage::Motion {
                delta: PointerDelta {
                    dx: i32::MIN,
                    dy: i32::MAX,
                },
                mods,
            }),
        ];
        for message in hot {
            assert!(message.is_hot_path() || message.is_release_all());
            let frame = Frame::new(message.clone(), Sequence(u32::MAX)).with_ack(
                crate::channel::ChannelId::ReliableInput,
                Ack {
                    cumulative: Sequence(u32::MAX),
                    bits: u32::MAX,
                },
            )
            // A época no pior caso: numa sessão de verdade ela é sorteada, e ocupa até 5 bytes.
            .in_epoch(crate::frame::Epoch(u32::MAX));
            let bytes = encode(&frame, Carrier::Udp).unwrap();
            assert!(
                bytes.len() <= limits::MAX_INPUT_MESSAGE,
                "{message:?} ocupou {} B, teto {} B",
                bytes.len(),
                limits::MAX_INPUT_MESSAGE
            );
        }
    }

    #[test]
    fn a_control_frame_fits_the_smallest_carrier() {
        let frame = Frame::new(Message::Control(Control::AckOnly), Sequence::ZERO);
        let bytes = encode(&frame, Carrier::Rfcomm).unwrap();
        assert!(bytes.len() < limits::MAX_RFCOMM_PLAINTEXT);
    }
}
