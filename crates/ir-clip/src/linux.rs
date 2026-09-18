//! O backend do Linux, e o que ele ainda não é.
//!
//! **Não está implementado**, e este arquivo existe para dizer isso com precisão em vez de deixar o
//! crate não compilar fora do Windows.
//!
//! # O que falta, e por que não é pouco
//!
//! No Wayland não há "o clipboard" alcançável de fora: quem lê e escreve precisa de uma ligação com
//! o compositor. [06, §6](../../../docs/06-linux.md) mapeia dois caminhos, e nenhum é uma chamada:
//!
//! | Caminho | Situação |
//! |---|---|
//! | `wlr-data-control` / `ext-data-control` | o único que permite ler fora de foco; **o GNOME não o expõe** |
//! | portal `org.freedesktop.portal.Clipboard` | exige estar dentro de uma sessão `RemoteDesktop` do portal |
//!
//! A bancada é Fedora com GNOME, então o caminho real ali é o portal — o que significa abrir uma
//! sessão de portal, mantê-la viva, e depender do D-Bus. É trabalho de tamanho próprio, e fingir
//! que existe seria pior que declarar que não.
//!
//! Enquanto isso, o Linux participa da transferência de arquivos **pelo pedido de envio** do canal
//! de controle, que não depende de clipboard nenhum. O que falta ali é só o gatilho automático.

use crate::error::{ClipError, Result};
use crate::{Clipboard, Vigia};

/// Abre o clipboard desta sessão.
///
/// # Errors
///
/// Sempre [`ClipError::Indisponivel`], hoje. Ver o topo do módulo.
pub fn abrir() -> Result<Box<dyn Clipboard>> {
    Err(ClipError::Indisponivel(
        "no Wayland o clipboard exige wlr-data-control ou o portal, e nenhum está ligado ainda",
    ))
}

/// Começa a vigiar as mudanças.
///
/// # Errors
///
/// Sempre [`ClipError::Indisponivel`], hoje.
pub fn vigiar() -> Result<Box<dyn Vigia>> {
    Err(ClipError::Indisponivel(
        "no Wayland o aviso de mudança de clipboard vem do compositor, e a ligação não existe ainda",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_ausencia_e_declarada_como_ausencia_e_nao_como_defeito() {
        // A diferença que o usuário vê: a interface diz "a sincronização de clipboard está
        // suspensa, e por quê", em vez de "erro".
        for erro in [
            abrir().err().expect("indisponível"),
            vigiar().err().expect("indisponível"),
        ] {
            assert!(erro.e_ausencia(), "{erro}");
            assert!(!erro.vale_repetir(), "repetir não faz o portal aparecer");
        }
    }

    #[test]
    fn a_mensagem_diz_o_que_falta_e_nao_so_que_falhou() {
        let erro = abrir().err().expect("indisponível").to_string();
        assert!(
            erro.contains("portal") || erro.contains("data-control"),
            "{erro}"
        );
    }
}
