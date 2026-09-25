//! O texto de um clipboard, que a sessão carrega sem nunca mostrar.

/// Texto de clipboard, dentro do limite do canal 4.
///
/// É o [`ir_proto::texto::TextoLimitado`] do protocolo, com o nome que a sessão sempre usou: o limite
/// e o `Debug` calado moram lá, num lugar só, e valem igual para o contrato com a interface.
pub type ClipText = ir_proto::texto::TextoLimitado;
