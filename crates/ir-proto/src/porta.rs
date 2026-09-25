//! A porta de rede do produto, num lugar só.

/// A porta em que o serviço escuta quando ninguém muda.
///
/// Origem: `docs/03-protocolo.md` §10. Mora aqui porque a interface completa com ela o IP digitado
/// sozinho, a mensagem de erro a usa de exemplo e a configuração nasce com ela — e as três precisam
/// dizer o mesmo número.
pub const DEFAULT_PORT: u16 = 52525;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_porta_padrao_e_a_do_documento_do_protocolo() {
        // Mudar este número desencontra máquinas já instaladas: o par procura na porta antiga.
        assert_eq!(DEFAULT_PORT, 52525);
    }
}
