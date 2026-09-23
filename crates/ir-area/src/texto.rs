//! O texto de um clipboard, que a sessão carrega sem nunca mostrar.

use ir_proto::limits::MAX_CLIPBOARD_TEXT_OFF_TCP;

/// Texto de clipboard, dentro do limite do canal 4.
///
/// Um tipo, e não `String`, por duas razões. O limite vira invariante: não existe `ClipText` maior
/// que [`MAX_CLIPBOARD_TEXT_OFF_TCP`], então quem monta a oferta não precisa conferir de novo. E o
/// `Debug` diz o tamanho, nunca o conteúdo — o clipboard carrega senha copiada do gerenciador de
/// senhas, e um `?input` num log não pode vazá-la (`docs/04-seguranca.md` §7).
#[derive(Clone, PartialEq, Eq)]
pub struct ClipText(String);

impl ClipText {
    /// O texto, se couber no canal 4.
    #[must_use]
    pub fn new(texto: String) -> Option<Self> {
        (texto.len() <= MAX_CLIPBOARD_TEXT_OFF_TCP).then_some(Self(texto))
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

impl std::fmt::Debug for ClipText {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ClipText({} bytes)", self.0.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn o_debug_nao_mostra_o_conteudo() {
        let texto = ClipText::new("senha-do-banco".to_owned()).unwrap();
        let visto = format!("{texto:?}");
        assert!(!visto.contains("senha"), "{visto}");
        assert!(visto.contains("14 bytes"), "{visto}");
    }

    #[test]
    fn texto_acima_do_limite_do_canal_nao_existe() {
        assert!(ClipText::new("a".repeat(MAX_CLIPBOARD_TEXT_OFF_TCP)).is_some());
        assert!(ClipText::new("a".repeat(MAX_CLIPBOARD_TEXT_OFF_TCP + 1)).is_none());
    }
}
