//! Uma pasta compartilhada no disco ([ADR-0015](../../../docs/adr/0015-pastas-compartilhadas.md)).
//!
//! O que o ajudante das pastas faz com o disco, sem a conversa: ver o que há ([`varredura`]), montar
//! ao lado e publicar com troca atômica, levar à lixeira ([`disco`]), guardar o índice
//! ([`guardado`]), e os trechos que vêm ([`baixa`]) e vão ([`envio`]) para o outro computador.
//!
//! Saiu do `ir-sincronia` quando ele passou do teto de 2 500 linhas de produção
//! ([09, §1](../../../docs/09-padroes-de-codigo.md)). A fronteira já estava provada: estes módulos
//! não conheciam o laço, a sessão com o par nem o serviço — só o motor (`ir-pasta`) e o disco.

#![cfg_attr(
    test,
    allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing
    )
)]

pub mod baixa;
pub mod disco;
pub mod envio;
pub mod guardado;
pub mod varredura;

use ir_proto::message::FolderMessage;

/// Para onde vão as mensagens ao par.
///
/// Um *trait*, e não o canal local direto, para o teste ligar dois ajudantes um ao outro sem
/// serviço no meio.
pub trait Saida {
    /// Manda uma mensagem ao par.
    fn enviar(&mut self, mensagem: FolderMessage);
}

impl Saida for Vec<FolderMessage> {
    fn enviar(&mut self, mensagem: FolderMessage) {
        self.push(mensagem);
    }
}
