//! Mensagens, codec e versionamento do protocolo InputRemote.
//!
//! Este crate é **puro**: nenhuma E/S, nenhum relógio, nenhuma API de sistema operacional,
//! nenhuma aleatoriedade. Ele traduz bytes em valores e valores em bytes, e nada mais.
//! A regra está em `docs/09-padroes-de-codigo.md` §3 e é verificada por
//! `cargo xtask check-purity`.
//!
//! O motivo dessa pureza não é elegância. O decodificador deste crate roda dentro de um
//! processo `SYSTEM`, processando bytes vindos de um rádio Bluetooth aberto, antes de haver
//! qualquer usuário logado na máquina. É o componente mais exposto do produto inteiro, e
//! precisa ser auditável sem nenhum contexto de execução.
//!
//! # Organização
//!
//! | Módulo | Responsabilidade |
//! |---|---|
//! | [`limits`] | tamanhos máximos, em um lugar só |
//! | [`carrier`] | os três portadores e o que cada um garante |
//! | [`version`] | versão do protocolo e negociação |
//! | [`ids`] | identificadores opacos (máquina, sessão, monitor) |
//! | [`input`] | teclas, botões, ponteiro, modificadores |
//! | [`screens`] | arranjo de telas e bordas |
//! | [`peer`] | o que uma ponta declara sobre si |
//! | [`channel`] | os seis canais lógicos |
//! | [`message`] | o catálogo de mensagens |
//! | [`frame`] | envelope de canal e sequência |
//! | [`codec`] | codificação e decodificação |
//! | [`error`] | o único tipo de erro do crate |
//!
//! # Compatibilidade
//!
//! O formato de fio é `postcard`, que **não é autodescritivo**: os campos são posicionais.
//! Não existe "campo desconhecido" para ignorar. Portanto qualquer mudança na ordem, no
//! tipo ou na quantidade de campos é quebra de compatibilidade silenciosa, e a única
//! proteção é a negociação de versão de [`version`] mais os vetores gravados em
//! `tests/vectors.rs`. Ver `docs/03-protocolo.md` §9.

#![forbid(unsafe_code)]
#![cfg_attr(
    test,
    allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)
)]

pub mod carrier;
pub mod channel;
pub mod codec;
pub mod error;
pub mod frame;
pub mod ids;
pub mod input;
pub mod limits;
pub mod message;
pub mod peer;
pub mod porta;
pub mod route;
pub mod screens;
pub mod texto;
pub mod version;

pub use carrier::Carrier;
pub use channel::{ChannelId, Reliability, Saturation};
pub use error::{ProtoError, Result};
pub use frame::Frame;
pub use frame::{Ack, ChannelAck, Epoch, Sequence};
pub use ids::{MachineId, MonitorId, SessionId};
pub use input::InputState;
pub use message::Message;
pub use peer::{Capabilities, MachineName, PrivilegedInputLevel};
pub use porta::DEFAULT_PORT;
pub use screens::{Edge, MonitorInfo, ScreenLayout};
pub use version::ProtocolVersion;
