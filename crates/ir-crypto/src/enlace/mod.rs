//! O enlace seguro que os portadores têm em comum: tudo o que fica entre o Noise e o socket.
//!
//! A rede (`ir-net`) e o rádio (`ir-bt`) não se enxergam — a seta de
//! [02, §2](../../../docs/02-arquitetura.md) impede um transporte de virar dependência do outro.
//! Por isso cada um tinha a sua cópia da mesma camada: os bytes de modo e de espécie, o
//! desenquadrador por prefixo de tamanho, o contador implícito, o fim do handshake e as duas
//! confirmações do pareamento. Cópias que já começavam a divergir (uma relatava a falha ao mandar a
//! confirmação, a outra a engolia).
//!
//! Aqui essa camada existe uma vez, e **pura**: nada de socket, de relógio nem de `tokio`. Os
//! transportes continuam donos do laço de E/S — esperar, reenviar, derrubar —, que é onde eles
//! realmente diferem.
//!
//! | Módulo | O quê |
//! |---|---|
//! | [`fio`] | o byte de modo do handshake e o byte de espécie do texto claro |
//! | [`quadro`] | o prefixo de tamanho dos portadores de *stream* |
//! | [`contador`] | o contador que as duas pontas contam, em vez de mandar |
//! | [`conclusao`] | como começar e como terminar um handshake |
//! | [`confirmacao`] | as duas confirmações do pareamento |

pub mod conclusao;
pub mod confirmacao;
pub mod contador;
pub mod fio;
pub mod quadro;

#[cfg(test)]
mod tests;

pub use conclusao::{ConnectMode, Established, concluir};
pub use confirmacao::{Confirmacao, Desfecho};
pub use contador::ContadorImplicito;
pub use fio::{Kind, Mode, corpo_de_handshake, desembrulhar, embrulhar, ler_handshake};
pub use quadro::{Desenquadrador, Excesso, enquadrar};
