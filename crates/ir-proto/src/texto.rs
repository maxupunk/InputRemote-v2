//! O texto de um clipboard: limitado ao canal 4 e calado nos registros.

use serde::{Deserialize, Serialize};

use crate::limits::MAX_CLIPBOARD_TEXT_OFF_TCP;

/// Texto de clipboard, dentro do limite do canal 4.
///
/// Um tipo, e não `String`, por duas razões. O limite vira invariante: não existe `TextoLimitado`
/// maior que [`MAX_CLIPBOARD_TEXT_OFF_TCP`], então quem monta a oferta não precisa conferir de novo
/// — e, como a conferência também vale **ao decodificar**, um texto maior mandado por um cliente
/// hostil não chega a existir do lado do serviço. E o `Debug` diz o tamanho, nunca o conteúdo: o
/// clipboard carrega senha copiada do gerenciador de senhas, e um `?input` num log não pode vazá-la
/// (`docs/04-seguranca.md` §7).
///
/// A sessão (`ir-area`) e o contrato com a interface (`ir-ipc`) usam este mesmo tipo: eram dois
/// embrulhos iguais, e um deles podia afrouxar o limite ou o `Debug` sem o outro perceber.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct TextoLimitado(String);

impl TextoLimitado {
    /// O maior texto que atravessa, em bytes.
    pub const MAX: usize = MAX_CLIPBOARD_TEXT_OFF_TCP;

    /// O texto, se couber no canal 4.
    #[must_use]
    pub fn new(texto: String) -> Option<Self> {
        (texto.len() <= Self::MAX).then_some(Self(texto))
    }

    /// O texto.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Devolve o texto.
    #[must_use]
    pub fn into_string(self) -> String {
        self.0
    }
}

impl TryFrom<String> for TextoLimitado {
    type Error = String;

    fn try_from(texto: String) -> Result<Self, Self::Error> {
        let tamanho = texto.len();
        Self::new(texto)
            .ok_or_else(|| format!("texto de clipboard com {tamanho} B, máximo {}", Self::MAX))
    }
}

impl From<TextoLimitado> for String {
    fn from(texto: TextoLimitado) -> Self {
        texto.0
    }
}

impl std::fmt::Debug for TextoLimitado {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "TextoLimitado({} B)", self.0.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn o_debug_nao_mostra_o_conteudo() {
        let texto = TextoLimitado::new("senha-do-banco".to_owned()).unwrap();
        let visto = format!("{texto:?}");
        assert!(!visto.contains("senha"), "{visto}");
        assert!(visto.contains("14 B"), "{visto}");
    }

    #[test]
    fn texto_acima_do_limite_do_canal_nao_existe() {
        assert!(TextoLimitado::new("a".repeat(TextoLimitado::MAX)).is_some());
        assert!(TextoLimitado::new("a".repeat(TextoLimitado::MAX + 1)).is_none());
    }

    #[test]
    fn texto_acima_do_limite_nao_decodifica() {
        // Montado à mão, como um cliente hostil faria: o tipo não deixa construir.
        let grande = "a".repeat(TextoLimitado::MAX + 1);
        let bytes = postcard::to_allocvec(&grande).unwrap();
        assert!(postcard::from_bytes::<TextoLimitado>(&bytes).is_err());
    }

    #[test]
    fn ida_e_volta_preserva_o_texto() {
        let texto = TextoLimitado::new("uma frase\ncom quebra".to_owned()).unwrap();
        let bytes = postcard::to_allocvec(&texto).unwrap();
        assert_eq!(
            postcard::from_bytes::<TextoLimitado>(&bytes).unwrap(),
            texto
        );
        assert_eq!(texto.as_str(), "uma frase\ncom quebra");
        assert_eq!(String::from(texto), "uma frase\ncom quebra");
    }
}
