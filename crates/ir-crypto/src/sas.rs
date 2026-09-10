//! O código de pareamento — *short authentication string*.
//!
//! Seis dígitos derivados do hash do handshake, iguais nos dois lados quando não há ninguém no
//! meio, diferentes quando há ([04, §3.2](../../../docs/04-seguranca.md)). O usuário compara os
//! dois códigos olhando as duas telas; é a única verificação, e ela **não é pulável**.

/// Rótulo de derivação, fixado na especificação do protocolo.
const LABEL: &str = "inputremote-sas-v1";

/// Deriva os seis dígitos do hash do handshake.
///
/// `blake3::derive_key` é um KDF de contexto separado, então dois hashes de handshake diferentes
/// produzem códigos independentes — que é o que faz o ataque de homem no meio aparecer como dois
/// códigos que não batem.
#[must_use]
pub(crate) fn code_from_handshake_hash(hash: &[u8]) -> [u8; 6] {
    let key = blake3::derive_key(LABEL, hash);
    // Seis dígitos decimais a partir dos primeiros bytes. `u32` mod 1_000_000 é uniforme o
    // bastante para uma comparação visual de segurança de canal.
    let raw = u32::from_le_bytes([key[0], key[1], key[2], key[3]]);
    let mut value = raw % 1_000_000;
    let mut digits = [0u8; 6];
    for slot in digits.iter_mut().rev() {
        *slot = u8::try_from(value % 10).unwrap_or(0);
        value /= 10;
    }
    digits
}

/// Compara dois códigos em tempo constante.
///
/// A comparação do código recebido é feita em tempo constante ([04, §3.2](../../../docs/04-seguranca.md)).
#[must_use]
pub fn codes_match(a: &[u8; 6], b: &[u8; 6]) -> bool {
    use subtle::ConstantTimeEq;
    a.ct_eq(b).into()
}

#[cfg(test)]
#[allow(clippy::indexing_slicing)]
mod tests {
    use super::*;

    #[test]
    fn the_same_hash_gives_the_same_code() {
        let hash = [7u8; 32];
        assert_eq!(
            code_from_handshake_hash(&hash),
            code_from_handshake_hash(&hash)
        );
    }

    #[test]
    fn different_hashes_give_different_codes() {
        let mut different = 0;
        for seed in 0u8..40 {
            let a = code_from_handshake_hash(&[seed; 32]);
            let b = code_from_handshake_hash(&[seed.wrapping_add(1); 32]);
            if a != b {
                different += 1;
            }
        }
        assert!(different > 35, "os códigos precisam variar com o hash");
    }

    #[test]
    fn every_digit_is_a_single_decimal() {
        for seed in 0u8..50 {
            for digit in code_from_handshake_hash(&[seed; 32]) {
                assert!(digit <= 9);
            }
        }
    }

    #[test]
    fn matching_is_exact() {
        let code = code_from_handshake_hash(&[1u8; 32]);
        assert!(codes_match(&code, &code));
        let mut other = code;
        other[0] = other[0].wrapping_add(1) % 10;
        assert!(!codes_match(&code, &other));
    }
}
