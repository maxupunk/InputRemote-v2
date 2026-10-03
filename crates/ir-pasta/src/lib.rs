//! O motor da pasta compartilhada, sem relógio, sem disco e sem rede
//! ([ADR-0015](../../../docs/adr/0015-pastas-compartilhadas.md)).
//!
//! Aqui moram as decisões da pasta — o que é conflito, quem fica com o nome, o que nunca viaja —,
//! para serem testadas em microssegundos, com dois nós simulados e um enlace que cai em qualquer
//! mensagem. Quem lê o disco, observa mudanças e fala com o serviço é o `ir-sincronia`, no ajudante
//! das pastas; este crate só responde.
//!
//! | Módulo | Decide |
//! |---|---|
//! | [`origem`] | o índice de quem compartilhou, e o que fazer com cada mudança da réplica |
//! | [`replica`] | o índice de quem recebeu, as mudanças locais que esperam e o que baixar |
//! | [`conflito`] | quem fica com o nome e como se chama a cópia de conflito |
//! | [`ignorar`] | o que nunca sincroniza: temporários de editor, arquivos do sistema |
//! | [`retrato`] | o que a varredura do disco mostrou |
//! | [`acao`] | o que o motor pede ao disco |

#![forbid(unsafe_code)]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

pub mod acao;
pub mod conflito;
pub mod ignorar;
pub mod origem;
pub mod replica;
pub mod retrato;

pub use acao::{Acao, Baixar};
pub use origem::{Contexto, Desfecho, EnvioRecebido, Origem};
pub use replica::{Operacao, Pendente, Replica};
pub use retrato::{Retrato, Visto};
