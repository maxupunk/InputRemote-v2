//! O ajudante das pastas compartilhadas: o disco, a vigia e a conversa com o serviço, em volta do
//! motor puro ([ADR-0015](../../../docs/adr/0015-pastas-compartilhadas.md)).
//!
//! Roda como o usuário, num processo próprio (`inputremote-agent --pastas`): quem grava na pasta do
//! usuário é ele, e não o serviço. O motor — o que mudou, o que é conflito, o que baixar — é o
//! `ir-pasta`; aqui só se executa o que ele decide e se fala com o outro lado, pelo serviço.
//!
//! | Módulo | Faz |
//! |---|---|
//! | [`laco`] | o laço do processo: ligar ao serviço, ouvir, varrer, andar |
//! | [`pastas`] | todas as pastas do usuário e a conversa de sessão com o par |
//! | [`viva`] | uma pasta em uso: o índice, o disco e o que está atravessando |
//! | [`vigia`] | o aviso de que algo mudou no disco |
//! | [`lugar`] | onde as coisas moram, e quem é este computador |
//! | `ir-acervo` | o disco: varredura, montagem, lixeira, índice guardado, trechos |
//! | [`atalho`] | o atalho na barra lateral do gerenciador de arquivos |

#![cfg_attr(
    test,
    allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing
    )
)]

pub mod atalho;
pub mod laco;
pub mod lugar;
pub mod pastas;
pub mod vigia;
pub mod viva;

// O disco da pasta mora no `ir-acervo`; os nomes de sempre continuam valendo aqui dentro.
pub use ir_acervo::{Saida, baixa, disco, envio, guardado, varredura};
pub use laco::{desregistrar, rodar};
pub use lugar::Lugar;
pub use pastas::Pastas;
