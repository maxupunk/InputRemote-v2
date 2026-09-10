//! O que uma ponta conta sobre si mesma no handshake.
//!
//! Nada aqui é confiança: capacidade anunciada é intenção, não permissão. Quem autoriza é
//! `ir-crypto` pela chave fixada e o serviço pelas políticas de
//! `docs/04-seguranca.md` §5 e §6. Estes campos servem para a interface explicar o que vai
//! e o que não vai funcionar, antes de o usuário descobrir na hora errada.

mod capabilities;
mod level;
mod name;

pub use capabilities::{Capabilities, ClipboardCapabilities};
pub use level::PrivilegedInputLevel;
pub use name::MachineName;
