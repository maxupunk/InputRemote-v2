//! Os desktops do Windows pelo nome, e qual deles é protegido — a regra num lugar só.
//!
//! O nome vem do sistema (`GetUserObjectInformationW`), e cada ponta o comparava do seu jeito: uma
//! com diferença de maiúsculas, outras sem. Aqui fica a única comparação; o agente a aplica e conta
//! ao serviço o resultado já decidido, em vez de o serviço reinterpretar o texto.
//!
//! Neutro de plataforma: fora do Windows não há desktops separados, e ninguém pergunta.

/// A área de trabalho do usuário: o único desktop que não é protegido.
pub const PADRAO: &str = "Default";

/// O desktop seguro: tela de bloqueio, de login, do Ctrl+Alt+Del e o UAC.
pub const SEGURO: &str = "Winlogon";

/// Se o desktop com este nome é protegido — tudo que não é a área de trabalho: a tela de bloqueio,
/// o UAC e o protetor de tela ([04, §6](../../../docs/04-seguranca.md)).
///
/// Sem diferença de maiúsculas: o Windows não diferencia nomes de objeto, e um `default` vindo do
/// sistema não pode virar "tela protegida".
#[must_use]
pub fn protegido(nome: &str) -> bool {
    !nome.eq_ignore_ascii_case(PADRAO)
}

/// Se, entre estes desktops, está o seguro — isto é, se quem injeta neles alcança a tela de
/// bloqueio e a de login.
#[must_use]
pub fn alcanca_o_seguro(desktops: &[String]) -> bool {
    desktops
        .iter()
        .any(|nome| nome.eq_ignore_ascii_case(SEGURO))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn so_a_area_de_trabalho_nao_e_protegida_com_qualquer_caixa() {
        assert!(!protegido(PADRAO));
        assert!(!protegido("default"), "o sistema não diferencia maiúsculas");
        assert!(protegido(SEGURO));
        assert!(protegido("Screen-saver"));
        assert!(protegido(""), "sem nome, na dúvida, protegido");
    }

    #[test]
    fn alcanca_o_seguro_so_com_o_winlogon_na_lista() {
        let com = vec![PADRAO.to_owned(), "winlogon".to_owned()];
        let sem = vec![PADRAO.to_owned(), "Screen-saver".to_owned()];
        assert!(alcanca_o_seguro(&com));
        assert!(!alcanca_o_seguro(&sem));
        assert!(!alcanca_o_seguro(&[]));
    }
}
