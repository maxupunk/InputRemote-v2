//! Criptografia e pareamento do InputRemote: Noise, código visual e identidades.
//!
//! Uma camada de criptografia só, para os três portadores ([03, §3](../../../docs/03-protocolo.md)).
//! Duas situações, dois padrões Noise:
//!
//! - [`Handshake::pair_initiator`] / [`Handshake::pair_responder`] — primeiro encontro, `Noise_XX`
//!   seguido do código de 6 dígitos que o usuário compara ([`sas`]).
//! - [`Handshake::reconnect_initiator`] / [`Handshake::reconnect_responder`] — toda sessão
//!   posterior, `Noise_IK` com a chave estática do par **fixada**.
//!
//! Depois do handshake, [`Transport`] cifra e decifra os quadros, com o contador explícito e a
//! janela de repetição de [`replay`].
//!
//! # Fronteira
//!
//! Este crate **não faz E/S de rede**. Ele transforma bytes em bytes: recebe uma mensagem de
//! handshake e devolve a resposta, recebe texto claro e devolve texto cifrado. Quem move os bytes
//! pelo socket é `ir-net`. É a mesma disciplina do núcleo — só que aqui `unsafe` continua
//! proibido e a aleatoriedade entra pela `OsRng`, a única E/S tolerada, e só para gerar chave.

#![forbid(unsafe_code)]
#![cfg_attr(
    test,
    allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::indexing_slicing,
        clippy::panic
    )
)]

pub mod error;
pub mod handshake;
pub mod identity;
pub mod replay;
pub mod sas;
pub mod transport;
pub mod turno;

pub use error::{CryptoError, Result};
pub use handshake::Handshake;
pub use identity::{Fingerprint, Identity, PublicKey, SecretBytes};
pub use replay::ReplayWindow;
pub use sas::codes_match;
pub use transport::{Opener, Sealer, Transport};
