//! Transporte Bluetooth RFCOMM do InputRemote.
//!
//! O portador **preferido** do produto para teclado e mouse
//! ([01, §5](../../../docs/01-visao-e-escopo.md)): a latência do rádio é mais constante que a de
//! uma rede local, e ele não depende de rede nenhuma estar de pé.
//!
//! # A mesma criptografia dos outros portadores
//!
//! Este crate **não tem criptografia própria**. Ele usa o mesmo [`ir_crypto`] que o `ir-net`:
//! `Noise_XX` com código de seis dígitos no primeiro encontro, `Noise_IK` com a chave fixada
//! depois, o mesmo contador explícito e a mesma janela de repetição. Uma camada L1 só para os
//! três portadores é a principal simplificação em relação ao v1
//! ([ADR-0003](../../../docs/adr/0003-noise-em-vez-de-quic.md)).
//!
//! # O que muda em relação ao UDP
//!
//! Só a camada 0, e a consequência dela na camada 2
//! ([03, §1](../../../docs/03-protocolo.md)):
//!
//! | | UDP | RFCOMM |
//! |---|---|---|
//! | Meio | datagrama, sem garantia | *stream* confiável e ordenado |
//! | Enquadramento | uma mensagem por datagrama | `u16` de tamanho + corpo |
//! | Texto claro máximo | 1 200 B | 512 B ([`ir_proto::limits::MAX_RFCOMM_PLAINTEXT`]) |
//! | Endereço | `SocketAddr` | [`BdAddr`] e um canal RFCOMM |
//!
//! Num *stream* não existe "um pacote, uma mensagem": os bytes chegam picados e colados. O
//! prefixo de tamanho é o que devolve a fronteira da mensagem, e sem ele o contador do Noise
//! começaria no meio de um texto cifrado.
//!
//! # `unsafe`
//!
//! O crate **não** usa `#![forbid(unsafe_code)]`, e isso é deliberado: o backend do Windows fala
//! Winsock `AF_BTH` por FFI, e [09, §4](../../../docs/09-padroes-de-codigo.md) autoriza `unsafe`
//! neste crate **apenas** em `windows::winsock`. Todo o resto — enquadramento, handshake,
//! máquina de estados do enlace — é seguro e testável sem rádio.

#![cfg_attr(
    test,
    allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::indexing_slicing,
        clippy::panic
    )
)]

pub mod addr;
pub mod bluez;
pub mod canal;
pub mod endpoint;
pub mod error;
pub mod handshake;
pub mod link;
pub mod radio;
pub mod wire;

#[cfg(target_os = "linux")]
pub mod linux;
#[cfg(windows)]
pub mod windows;

/// O rádio desta máquina, seja ela qual for.
///
/// Um apelido por plataforma, para o serviço não precisar de `#[cfg]` nenhum: ele pede
/// [`abrir_radio`] e recebe algo que implementa [`Radio`]. É a mesma forma de
/// `ir_input::open_injector`, e pela mesma razão.
#[cfg(target_os = "linux")]
pub type RadioDoSistema = linux::RadioLinux;

/// O rádio desta máquina, seja ela qual for.
#[cfg(windows)]
pub type RadioDoSistema = windows::RadioWindows;

/// Abre o rádio Bluetooth desta máquina, com a escuta já no ar.
///
/// # Errors
///
/// [`BtError::SemRadio`] se não houver rádio utilizável — o caso em que o produto **não** deve
/// insistir: ele degrada para a rede e diz o motivo. [`BtError::Io`] se o canal do produto já
/// estiver ocupado.
#[cfg(any(windows, target_os = "linux"))]
pub fn abrir_radio() -> Result<RadioDoSistema> {
    #[cfg(target_os = "linux")]
    {
        linux::RadioLinux::abrir()
    }
    #[cfg(windows)]
    {
        windows::RadioWindows::abrir()
    }
}

pub use addr::{BdAddr, CANAL};
pub use canal::{Canal, Quadros};
pub use endpoint::{BtCommand, BtEvent, Endpoint, EndpointHandle};
pub use error::{BtError, Result};
pub use handshake::{ConnectMode, Established};
pub use link::EnlaceSeguro;
pub use radio::{Dispositivo, Radio};
pub use wire::{Kind, Mode};
