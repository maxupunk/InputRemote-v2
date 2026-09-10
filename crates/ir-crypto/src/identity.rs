//! A identidade estática da máquina: um par de chaves X25519.
//!
//! É da **máquina**, não do usuário — o serviço precisa dela antes de haver usuário
//! ([04, §3.1](../../../docs/04-seguranca.md)). A privada nunca sai daqui em claro, e é apagada
//! da memória na queda por `zeroize`.

use x25519_dalek::{PublicKey as DalekPublic, StaticSecret};
use zeroize::{Zeroize, ZeroizeOnDrop};

use crate::error::{CryptoError, Result};

/// A chave pública estática de uma máquina, como viaja e como é comparada.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PublicKey(pub [u8; 32]);

impl PublicKey {
    /// A impressão digital exibida ao usuário: BLAKE3 da chave, em grupos de 4 caracteres.
    ///
    /// Base32 sem os caracteres ambíguos, porque uma pessoa vai ler isto em voz alta comparando
    /// com a outra tela ([04, §3.1](../../../docs/04-seguranca.md)).
    #[must_use]
    pub fn fingerprint(&self) -> Fingerprint {
        Fingerprint::from_hash(blake3::hash(&self.0).as_bytes())
    }
}

/// A impressão digital legível de uma chave pública: 5 grupos de 4 caracteres.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Fingerprint([u8; 24]);

/// Base32 de Crockford sem os caracteres ambíguos (sem I, L, O, U).
const ALPHABET: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";

impl Fingerprint {
    fn from_hash(hash: &[u8; 32]) -> Self {
        // 20 caracteres (100 bits) bastam para comparação visual; em 5 grupos de 4 com espaço,
        // são 24 bytes de texto.
        let mut out = [b' '; 24];
        let mut cursor = 0;
        for pos in 0..20usize {
            let index = usize::from(extract5(hash, pos * 5));
            let symbol = ALPHABET.get(index).copied().unwrap_or(b'0');
            if let Some(slot) = out.get_mut(cursor) {
                *slot = symbol;
            }
            cursor += 1;
            if pos % 4 == 3 && pos != 19 {
                cursor += 1; // deixa o espaço já presente
            }
        }
        Self(out)
    }

    /// O texto da impressão digital.
    #[must_use]
    pub fn as_str(&self) -> &str {
        core::str::from_utf8(&self.0).unwrap_or("????")
    }
}

/// Extrai 5 bits a partir do bit `offset`, big-endian.
fn extract5(bytes: &[u8; 32], offset: usize) -> u8 {
    let mut value = 0u8;
    for i in 0..5 {
        let bit = offset + i;
        let byte = bytes.get(bit / 8).copied().unwrap_or(0);
        let set = (byte >> (7 - (bit % 8))) & 1;
        value = (value << 1) | set;
    }
    value
}

impl core::fmt::Display for Fingerprint {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// O par de chaves estático desta máquina.
///
/// O segredo some da memória quando a `Identity` cai (`ZeroizeOnDrop`).
#[derive(Clone, ZeroizeOnDrop)]
pub struct Identity {
    secret: [u8; 32],
    #[zeroize(skip)]
    public: [u8; 32],
}

impl Identity {
    /// Gera uma identidade nova, com aleatoriedade do sistema.
    #[must_use]
    pub fn generate() -> Self {
        let secret = StaticSecret::random_from_rng(rand::rngs::OsRng);
        Self::from_static_secret(&secret)
    }

    /// Reconstrói a identidade a partir da chave privada gravada.
    ///
    /// A pública é **derivada**, nunca lida de disco: assim a impressão digital que o usuário vê
    /// e a chave que o par fixa são sempre a mesma, mesmo que o arquivo tenha sido adulterado.
    ///
    /// # Errors
    ///
    /// [`CryptoError::BadKeyLength`] se o material não tiver 32 bytes.
    pub fn from_secret_bytes(secret: &[u8]) -> Result<Self> {
        let bytes: [u8; 32] = secret.try_into().map_err(|_| CryptoError::BadKeyLength)?;
        Ok(Self::from_static_secret(&StaticSecret::from(bytes)))
    }

    fn from_static_secret(secret: &StaticSecret) -> Self {
        let public = DalekPublic::from(secret);
        Self {
            secret: secret.to_bytes(),
            public: public.to_bytes(),
        }
    }

    /// A chave pública, para anunciar e para o par fixar.
    #[must_use]
    pub fn public(&self) -> PublicKey {
        PublicKey(self.public)
    }

    /// A impressão digital desta máquina.
    #[must_use]
    pub fn fingerprint(&self) -> Fingerprint {
        self.public().fingerprint()
    }

    /// A chave privada em bytes, para o handshake e para a persistência.
    ///
    /// É o único ponto em que o segredo sai daqui; quem grava em disco deve restringir a ACL a
    /// `SYSTEM`/`root` ([02, §7](../../../docs/02-arquitetura.md)).
    #[must_use]
    pub(crate) fn secret(&self) -> &[u8; 32] {
        &self.secret
    }

    /// Uma cópia efêmera do segredo para gravação, apagada da memória ao cair.
    #[must_use]
    pub fn secret_bytes(&self) -> SecretBytes {
        SecretBytes(self.secret)
    }
}

impl core::fmt::Debug for Identity {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Identity")
            .field("fingerprint", &self.fingerprint())
            .finish_non_exhaustive()
    }
}

/// Uma cópia efêmera da chave privada, apagada da memória ao cair.
///
/// `Debug` é manual e não imprime os bytes: é material de chave.
#[derive(Zeroize, ZeroizeOnDrop)]
pub struct SecretBytes([u8; 32]);

impl core::fmt::Debug for SecretBytes {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("SecretBytes(oculto)")
    }
}

impl SecretBytes {
    /// Os bytes, para gravar em disco.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_saved_secret_round_trips_to_the_same_public() {
        let original = Identity::generate();
        let restored = Identity::from_secret_bytes(original.secret_bytes().as_bytes()).unwrap();
        assert_eq!(original.public(), restored.public());
        assert_eq!(original.fingerprint(), restored.fingerprint());
    }

    #[test]
    fn distinct_identities_have_distinct_fingerprints() {
        let a = Identity::generate();
        let b = Identity::generate();
        assert_ne!(a.public(), b.public());
        assert_ne!(a.fingerprint(), b.fingerprint());
    }

    #[test]
    fn the_fingerprint_is_grouped_and_unambiguous() {
        let text = Identity::generate().fingerprint().as_str().to_owned();
        assert_eq!(text.split(' ').count(), 5, "{text}");
        for group in text.split(' ') {
            assert_eq!(group.len(), 4, "{text}");
            for ch in group.bytes() {
                assert!(ALPHABET.contains(&ch), "{ch} fora do alfabeto");
            }
        }
    }

    #[test]
    fn a_short_secret_is_refused() {
        assert_eq!(
            Identity::from_secret_bytes(&[0u8; 16]).unwrap_err(),
            CryptoError::BadKeyLength
        );
    }
}
