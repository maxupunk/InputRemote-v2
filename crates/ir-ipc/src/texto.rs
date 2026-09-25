//! O texto do clipboard no contrato: limitado e calado.

/// Texto copiado, a caminho do par ou vindo dele.
///
/// É o [`ir_proto::texto::TextoLimitado`] do protocolo: o limite é o do canal 4 e é conferido
/// **ao decodificar**, e não por quem recebe — um texto maior não chega a existir do lado do
/// serviço. E o `Debug` diz o tamanho, nunca o conteúdo ([04, §7](../../../docs/04-seguranca.md)).
pub type TextoDoClipboard = ir_proto::texto::TextoLimitado;

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::{Pedido, codec};

    #[test]
    fn o_debug_nao_mostra_o_texto() {
        let pedido = Pedido::OferecerTexto(TextoDoClipboard::new("senha".to_owned()).unwrap());
        let visto = format!("{pedido:?}");
        assert!(!visto.contains("senha"), "{visto}");
    }

    #[test]
    fn o_maior_texto_cabe_numa_mensagem_de_ipc() {
        let texto = TextoDoClipboard::new("ç".repeat(TextoDoClipboard::MAX / 2)).unwrap();
        let bytes = codec::codificar(&Pedido::OferecerTexto(texto.clone())).unwrap();
        let corpo = &bytes[codec::PREFIXO..];
        assert_eq!(
            codec::decodificar::<Pedido>(corpo).unwrap(),
            Pedido::OferecerTexto(texto)
        );
    }
}
