//! O enquadramento de um datagrama UDP. Parte pura, sem socket.
//!
//! Dois formatos, separados pela fase do enlace (nunca ambíguos, porque quem lê sabe se está em
//! handshake ou já estabelecido):
//!
//! - **Handshake** — `[modo: u8][mensagem Noise]`. O modo (parear ou reconectar) vai em claro
//!   porque quem recebe o primeiro datagrama precisa escolher o padrão Noise antes de decifrar
//!   qualquer coisa.
//! - **Estabelecido** — `[contador: u64 LE][texto cifrado]`. O contador é o nonce, e precisa
//!   estar em claro para o outro lado decifrar ([03, §3.1](../../../docs/03-protocolo.md)).
//!
//! O texto claro cifrado começa com um byte de espécie ([`Kind`]), autenticado junto com o
//! resto: assim um marcador de pareamento não pode ser forjado nem confundido com um quadro.
//!
//! Os dois bytes — modo e espécie — são os mesmos do rádio e moram no
//! [`ir_crypto::enlace`]; daqui só sai o que é do datagrama: o contador em claro.

/// O modo e a espécie, e o embrulho de cada um: os mesmos do rádio, e por isso no `ir-crypto`.
///
/// Os nomes daqui são os que o UDP sempre usou — um corpo de handshake é um datagrama inteiro.
pub use ir_crypto::enlace::{
    Kind, Mode, corpo_de_handshake as handshake_datagram, desembrulhar as unwrap,
    embrulhar as wrap, ler_handshake as parse_handshake,
};

/// Tamanho do prefixo de contador, em bytes.
pub const COUNTER_LEN: usize = 8;

/// Monta um datagrama de dados: `[contador LE][texto cifrado]`.
#[must_use]
pub fn data_datagram(counter: u64, ciphertext: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(COUNTER_LEN + ciphertext.len());
    out.extend_from_slice(&counter.to_le_bytes());
    out.extend_from_slice(ciphertext);
    out
}

/// Separa um datagrama de dados em contador e texto cifrado.
#[must_use]
pub fn parse_data(datagram: &[u8]) -> Option<(u64, &[u8])> {
    let prefix = datagram.get(..COUNTER_LEN)?;
    let bytes: [u8; COUNTER_LEN] = prefix.try_into().ok()?;
    let ciphertext = datagram.get(COUNTER_LEN..)?;
    Some((u64::from_le_bytes(bytes), ciphertext))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn handshake_round_trips() {
        let d = handshake_datagram(Mode::Pair, b"noise-msg");
        assert_eq!(parse_handshake(&d), Some((Mode::Pair, &b"noise-msg"[..])));
    }

    #[test]
    fn data_round_trips() {
        let d = data_datagram(0x0102_0304_0506_0708, b"cipher");
        assert_eq!(
            parse_data(&d),
            Some((0x0102_0304_0506_0708, &b"cipher"[..]))
        );
    }

    #[test]
    fn wrap_round_trips() {
        let w = wrap(Kind::PairConfirm, b"");
        assert_eq!(unwrap(&w), Some((Kind::PairConfirm, &b""[..])));
        let w = wrap(Kind::SessionFrame, b"frame");
        assert_eq!(unwrap(&w), Some((Kind::SessionFrame, &b"frame"[..])));
    }

    #[test]
    fn a_short_data_datagram_is_rejected() {
        assert_eq!(parse_data(&[0, 1, 2]), None);
    }

    #[test]
    fn an_empty_handshake_datagram_is_rejected() {
        assert_eq!(parse_handshake(&[]), None);
    }

    #[test]
    fn unknown_mode_and_kind_bytes_are_rejected() {
        assert_eq!(Mode::from_byte(9), None);
        assert_eq!(Kind::from_byte(9), None);
        assert_eq!(unwrap(&[9, 0, 0]), None);
    }

    #[test]
    fn the_modes_and_kinds_round_trip_through_bytes() {
        for mode in [Mode::Pair, Mode::Reconnect] {
            assert_eq!(Mode::from_byte(mode.to_byte()), Some(mode));
        }
        for kind in [Kind::SessionFrame, Kind::PairConfirm, Kind::PairReject] {
            assert_eq!(Kind::from_byte(kind.to_byte()), Some(kind));
        }
    }
}
