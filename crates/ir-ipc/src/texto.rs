//! O texto do clipboard no contrato: limitado e calado.

use ir_proto::limits::MAX_CLIPBOARD_TEXT_OFF_TCP;
use serde::{Deserialize, Serialize};

/// Texto copiado, a caminho do par ou vindo dele.
///
/// O limite é o do canal 4 e é conferido **ao decodificar**, e não por quem recebe: um texto maior
/// não chega a existir do lado do serviço. E o `Debug` diz o tamanho, nunca o conteúdo — o que se
/// copia inclui a senha tirada do gerenciador de senhas, e um `?pedido` num log não pode levá-la
/// ([04, §7](../../../docs/04-seguranca.md)).
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct TextoDoClipboard(String);

impl TextoDoClipboard {
    /// O maior texto que atravessa.
    pub const MAXIMO: usize = MAX_CLIPBOARD_TEXT_OFF_TCP;

    /// O texto, se couber.
    #[must_use]
    pub fn novo(texto: String) -> Option<Self> {
        (texto.len() <= Self::MAXIMO).then_some(Self(texto))
    }

    /// O texto.
    #[must_use]
    pub fn como_str(&self) -> &str {
        &self.0
    }

    /// Devolve o texto.
    #[must_use]
    pub fn em_string(self) -> String {
        self.0
    }
}

impl TryFrom<String> for TextoDoClipboard {
    type Error = String;

    fn try_from(texto: String) -> Result<Self, Self::Error> {
        let tamanho = texto.len();
        Self::novo(texto).ok_or_else(|| {
            format!(
                "texto de clipboard com {tamanho} B, máximo {}",
                Self::MAXIMO
            )
        })
    }
}

impl From<TextoDoClipboard> for String {
    fn from(texto: TextoDoClipboard) -> Self {
        texto.0
    }
}

impl std::fmt::Debug for TextoDoClipboard {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "TextoDoClipboard({} B)", self.0.len())
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::{Pedido, codec};

    #[test]
    fn o_debug_nao_mostra_o_texto() {
        let pedido = Pedido::OferecerTexto(TextoDoClipboard::novo("senha".to_owned()).unwrap());
        let visto = format!("{pedido:?}");
        assert!(!visto.contains("senha"), "{visto}");
    }

    #[test]
    fn texto_acima_do_limite_nao_decodifica() {
        // Montado à mão, como um cliente hostil faria: o tipo não deixa construir.
        let grande: String = "a".repeat(TextoDoClipboard::MAXIMO + 1);
        let bytes = postcard::to_allocvec(&grande).unwrap();
        assert!(postcard::from_bytes::<TextoDoClipboard>(&bytes).is_err());
    }

    #[test]
    fn o_maior_texto_cabe_numa_mensagem_de_ipc() {
        let texto = TextoDoClipboard::novo("ç".repeat(TextoDoClipboard::MAXIMO / 2)).unwrap();
        let bytes = codec::codificar(&Pedido::OferecerTexto(texto.clone())).unwrap();
        let corpo = &bytes[codec::PREFIXO..];
        assert_eq!(
            codec::decodificar::<Pedido>(corpo).unwrap(),
            Pedido::OferecerTexto(texto)
        );
    }
}
