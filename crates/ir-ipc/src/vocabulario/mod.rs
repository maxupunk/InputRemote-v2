//! O vocabulário da interface, independente do protocolo.
//!
//! Estes tipos existem porque `ir-ui` **não pode** depender de `ir-proto`
//! ([02, §2](../../../docs/02-arquitetura.md)). A regra não é burocrática: se os tipos que a tela
//! desenha forem os tipos do fio, então mudar o formato de fio quebra a interface, e a interface
//! passa a ter opinião sobre protocolo. Foi assim que o v1 acabou com o crate da interface maior
//! que transporte e plataforma somados.
//!
//! Aqui cada tipo é o mesmo conceito visto do lado do usuário, com o nome que ele reconhece —
//! "Bluetooth", e não RFCOMM — e com as conversões de e para o protocolo num lugar só. O serviço
//! traduz; a interface nunca vê o outro lado.
//!
//! O módulo [`crate::agent`] é a exceção deliberada: o canal do agente carrega injeção de entrada,
//! e ali os tipos do protocolo são exatamente os certos.

pub mod capacidade;
pub mod identidade;
pub mod portador;

pub use capacidade::{Clipboard, Nivel, Recursos};
pub use identidade::{Maquina, Nome};
pub use portador::{Borda, Portador};
