//! Transporte de rede do InputRemote: UDP de entrada, cifrado, e descoberta mDNS.
//!
//! Este crate move os bytes que `ir-crypto` cifrou. Ele não conhece a máquina de estados da
//! sessão nem o significado dos quadros — recebe bytes de `ir_proto::Frame` já codificados e os
//! entrega ao par, e faz o caminho de volta.
//!
//! O ponto de entrada é o [`Endpoint`]: uma tarefa `tokio` dona do socket, que fala com o
//! serviço por dois canais ([`NetCommand`] entra, [`NetEvent`] sai). O serviço nunca toca no
//! socket; ele manda "conecte", "envie este quadro", "o usuário confirmou", e recebe "aqui está
//! o código", "estabelecido", "chegou um quadro", "caiu".
//!
//! # O que este crate garante
//!
//! - **Nada de sessão trafega antes das duas confirmações de pareamento** — o endpoint só emite
//!   [`NetEvent::Established`] quando os dois lados confirmaram o código
//!   ([04, §3.2](../../../docs/04-seguranca.md)).
//! - **Um datagrama que não abre é ignorado, não derruba o enlace** — senão um pacote solto de
//!   qualquer um seria negação de serviço.

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

pub mod bulk;
pub mod discovery;
pub mod endpoint;
pub mod error;
pub mod handshake;
pub mod link;
pub mod turno;
pub mod wire;

pub use discovery::{Candidate, Discovery};
pub use endpoint::{Endpoint, EndpointHandle, NetCommand, NetEvent, bind};
pub use error::{NetError, Result};
pub use handshake::{ConnectMode, Established};
pub use wire::{Kind, Mode};
