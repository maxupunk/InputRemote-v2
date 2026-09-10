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

/// O modo de um handshake, no primeiro byte do primeiro datagrama.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Mode {
    /// Primeiro pareamento — `Noise_XX`, seguido do código de 6 dígitos.
    Pair = 0,
    /// Reconexão — `Noise_IK`, com a chave do par fixada.
    Reconnect = 1,
}

impl Mode {
    /// Lê o modo do primeiro byte de um datagrama de handshake.
    #[must_use]
    pub const fn from_byte(byte: u8) -> Option<Self> {
        match byte {
            0 => Some(Self::Pair),
            1 => Some(Self::Reconnect),
            _ => None,
        }
    }

    /// O byte que representa este modo.
    #[must_use]
    pub const fn to_byte(self) -> u8 {
        self as u8
    }
}

/// A espécie do texto claro cifrado, no primeiro byte de dentro do envelope.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Kind {
    /// Um quadro do protocolo (`ir_proto::Frame` codificado).
    SessionFrame = 0,
    /// O usuário confirmou que os códigos batem.
    PairConfirm = 1,
    /// O usuário disse que os códigos não batem, ou recusou.
    PairReject = 2,
}

impl Kind {
    /// Lê a espécie do primeiro byte do texto claro.
    #[must_use]
    pub const fn from_byte(byte: u8) -> Option<Self> {
        match byte {
            0 => Some(Self::SessionFrame),
            1 => Some(Self::PairConfirm),
            2 => Some(Self::PairReject),
            _ => None,
        }
    }

    /// O byte que representa esta espécie.
    #[must_use]
    pub const fn to_byte(self) -> u8 {
        self as u8
    }
}

/// Tamanho do prefixo de contador, em bytes.
pub const COUNTER_LEN: usize = 8;

/// Monta um datagrama de handshake: `[modo][mensagem]`.
#[must_use]
pub fn handshake_datagram(mode: Mode, message: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(1 + message.len());
    out.push(mode.to_byte());
    out.extend_from_slice(message);
    out
}

/// Separa um datagrama de handshake em modo e mensagem.
#[must_use]
pub fn parse_handshake(datagram: &[u8]) -> Option<(Mode, &[u8])> {
    let (first, rest) = datagram.split_first()?;
    Some((Mode::from_byte(*first)?, rest))
}

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

/// Embrulha um texto claro com o byte de espécie, para cifrar.
#[must_use]
pub fn wrap(kind: Kind, payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(1 + payload.len());
    out.push(kind.to_byte());
    out.extend_from_slice(payload);
    out
}

/// Separa um texto claro decifrado em espécie e conteúdo.
#[must_use]
pub fn unwrap(plaintext: &[u8]) -> Option<(Kind, &[u8])> {
    let (first, rest) = plaintext.split_first()?;
    Some((Kind::from_byte(*first)?, rest))
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
