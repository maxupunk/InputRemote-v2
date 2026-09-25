//! Os dois bytes em claro — ou quase — que todo portador põe em volta do Noise.
//!
//! - **Modo** — o primeiro byte de um corpo de handshake: `[modo: u8][mensagem Noise]`. Vai em
//!   claro porque quem recebe o primeiro corpo precisa escolher o padrão Noise antes de decifrar
//!   qualquer coisa.
//! - **Espécie** — o primeiro byte do texto claro, **dentro** do envelope cifrado:
//!   `[espécie: u8][conteúdo]`. Autenticada junto com o resto, então um marcador de pareamento não
//!   pode ser forjado nem confundido com um quadro de sessão.
//!
//! Estes bytes são formato de fio: os valores não mudam sem incremento de versão do protocolo.

/// O modo de um handshake, no primeiro byte do primeiro corpo.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Mode {
    /// Primeiro pareamento — `Noise_XX`, seguido do código de 6 dígitos.
    Pair = 0,
    /// Reconexão — `Noise_IK`, com a chave do par fixada.
    Reconnect = 1,
    /// Troca de chaves de um enlace de pé — `Noise_IK`, como a reconexão, mas a sessão não cai.
    ///
    /// Só a rede troca chaves. Um par que não conhece este modo ignora o corpo, e o enlace atual
    /// segue valendo; o rádio o recusa como malformado.
    Rekey = 2,
}

impl Mode {
    /// Lê o modo do primeiro byte de um corpo de handshake.
    #[must_use]
    pub const fn from_byte(byte: u8) -> Option<Self> {
        match byte {
            0 => Some(Self::Pair),
            1 => Some(Self::Reconnect),
            2 => Some(Self::Rekey),
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

/// Monta o corpo de um handshake: `[modo][mensagem Noise]`.
#[must_use]
pub fn corpo_de_handshake(modo: Mode, mensagem: &[u8]) -> Vec<u8> {
    com_byte_na_frente(modo.to_byte(), mensagem)
}

/// Separa um corpo de handshake em modo e mensagem.
#[must_use]
pub fn ler_handshake(corpo: &[u8]) -> Option<(Mode, &[u8])> {
    let (primeiro, resto) = corpo.split_first()?;
    Some((Mode::from_byte(*primeiro)?, resto))
}

/// Embrulha um texto claro com o byte de espécie, para cifrar.
#[must_use]
pub fn embrulhar(especie: Kind, conteudo: &[u8]) -> Vec<u8> {
    com_byte_na_frente(especie.to_byte(), conteudo)
}

/// Separa um texto claro decifrado em espécie e conteúdo.
#[must_use]
pub fn desembrulhar(texto_claro: &[u8]) -> Option<(Kind, &[u8])> {
    let (primeiro, resto) = texto_claro.split_first()?;
    Some((Kind::from_byte(*primeiro)?, resto))
}

fn com_byte_na_frente(byte: u8, resto: &[u8]) -> Vec<u8> {
    let mut saida = Vec::with_capacity(1 + resto.len());
    saida.push(byte);
    saida.extend_from_slice(resto);
    saida
}
