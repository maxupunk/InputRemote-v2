//! Confiabilidade dos canais confiáveis sobre datagrama.
//!
//! Sobre RFCOMM e TCP este módulo não é usado: o portador já entrega ordenado e completo.
//! Sobre UDP ele é obrigatório, e é pequeno de propósito — não é uma reimplementação de TCP,
//! é o mínimo que `docs/03-protocolo.md` §4.1 descreve:
//!
//! - sequência por canal;
//! - confirmação cumulativa mais bitmap das 32 anteriores;
//! - retransmissão após `RTO = max(20 ms, 2 × srtt)`;
//! - no máximo 5 tentativas, e então o enlace cai.
//!
//! **Por que cair em vez de continuar.** Esgotadas as tentativas, prosseguir significaria
//! seguir com uma lacuna no canal de teclado. Se a mensagem perdida for um `KeyUp`, a tecla
//! fica presa na máquina do outro. Cair é ruim; tecla presa é pior, porque o usuário não sabe
//! o que aconteceu nem como sair.

mod channels;
mod receiver;
mod sender;

pub use channels::{Due, ReliableChannels};
pub use receiver::{Delivery, Receiver};
pub use sender::{SendOutcome, Sender, TimeoutOutcome, WINDOW};
