//! Identificadores opacos.
//!
//! Todos são tipos distintos, e nenhum é um alias de inteiro. Trocar um identificador de
//! sessão por um de monitor é o tipo de erro que o compilador tem de pegar, não o teste de
//! integração.
//!
//! Nenhum identificador daqui é derivado de nome de usuário, de endereço ou de material de
//! chave — `docs/03-protocolo.md` §10 e `docs/04-seguranca.md` §7.

use serde::{Deserialize, Serialize};

/// Identificador estável de uma máquina, do ponto de vista do protocolo.
///
/// Gerado por CSPRNG na primeira subida do serviço e persistido. Não é derivado de MAC, de
/// número de série nem de nome de usuário: ele identifica a instalação, não a pessoa nem o
/// hardware.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct MachineId(pub [u8; 16]);

impl MachineId {
    /// Representação curta e legível, para a interface e para os logs.
    ///
    /// Oito dígitos hexadecimais dos primeiros quatro bytes. É identificação, não
    /// autenticação — quem autentica é a chave estática de `ir-crypto`.
    #[must_use]
    pub fn short(&self) -> ShortId {
        let mut out = [0u8; 8];
        for (slot, byte) in out.chunks_exact_mut(2).zip(self.0.iter().take(4)) {
            if let [high, low] = slot {
                *high = hex_digit(byte >> 4);
                *low = hex_digit(byte & 0x0F);
            }
        }
        ShortId(out)
    }
}

/// Identificador de sessão, único por sessão estabelecida.
///
/// Aparece em todo log para correlacionar os três processos e as duas máquinas
/// (`docs/09-padroes-de-codigo.md` §8).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SessionId(pub u64);

/// Identificador de monitor dentro de um arranjo anunciado.
///
/// Válido apenas no contexto do `ScreenLayout` que o anunciou. Não é o identificador do
/// sistema operacional, que difere entre plataformas e entre reconexões.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct MonitorId(pub u8);

/// Oito caracteres hexadecimais, sem alocação.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct ShortId([u8; 8]);

impl core::fmt::Display for ShortId {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        // Todo byte foi produzido por `hex_digit`, logo é ASCII válido.
        match core::str::from_utf8(&self.0) {
            Ok(text) => f.write_str(text),
            Err(_) => f.write_str("????????"),
        }
    }
}

impl core::fmt::Debug for ShortId {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "ShortId({self})")
    }
}

const fn hex_digit(nibble: u8) -> u8 {
    match nibble {
        0..=9 => b'0' + nibble,
        10..=15 => b'a' + (nibble - 10),
        _ => b'?',
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_id_renders_the_first_four_bytes() {
        let id = MachineId([
            0xDE, 0xAD, 0xBE, 0xEF, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0A,
            0x0B, 0x0C,
        ]);
        assert_eq!(id.short().to_string(), "deadbeef");
    }

    #[test]
    fn short_id_pads_low_nibbles() {
        let id = MachineId([0x00, 0x0F, 0xF0, 0x01, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        assert_eq!(id.short().to_string(), "000ff001");
    }

    #[test]
    fn identifiers_are_distinct_types() {
        // Não compila se alguém transformar um deles em alias. O valor do teste é
        // justamente falhar a build nesse caso.
        let machine = MachineId([0; 16]);
        let session = SessionId(1);
        let monitor = MonitorId(0);
        assert_eq!(core::mem::size_of_val(&machine), 16);
        assert_eq!(core::mem::size_of_val(&session), 8);
        assert_eq!(core::mem::size_of_val(&monitor), 1);
    }
}
