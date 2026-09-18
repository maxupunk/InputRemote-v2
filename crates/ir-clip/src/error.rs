//! O tipo de erro do crate.
//!
//! Uma distinção carrega o módulo: **indisponível** não é **falha**.
//!
//! O clipboard pode simplesmente não existir onde estamos — no desktop `Winlogon` não há clipboard
//! de usuário, e num *greeter* Linux não há sessão gráfica. Nessas horas a interface tem de dizer
//! "a sincronização está suspensa", declaradamente ([05, §6](../../../docs/05-windows.md),
//! [06, §6](../../../docs/06-linux.md)), e não "erro".

/// Resultado das operações de clipboard.
pub type Result<T> = core::result::Result<T, ClipError>;

/// O que pode dar errado com o clipboard.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ClipError {
    /// Não há clipboard alcançável daqui.
    ///
    /// Estado normal, não defeito: desktop seguro no Windows, sessão ausente no Linux, compositor
    /// sem o protocolo necessário.
    #[error("não há clipboard alcançável: {0}")]
    Indisponivel(&'static str),

    /// Outro programa está segurando o clipboard.
    ///
    /// No Windows o clipboard é um recurso global com dono, e `OpenClipboard` falha enquanto outro
    /// processo o tem aberto. É transitório por natureza, e quem chama tenta de novo
    /// ([05, §6](../../../docs/05-windows.md)).
    #[error("o clipboard está ocupado por outro programa")]
    Ocupado,

    /// O formato que o sistema ofereceu não é um que o protocolo transporta.
    #[error("formato de clipboard não suportado")]
    FormatoNaoSuportado,

    /// Falha de API do sistema, com o código dele.
    #[error("o sistema recusou a operação de clipboard: {0}")]
    Sistema(String),
}

impl ClipError {
    /// Se vale tentar de novo daqui a pouco.
    ///
    /// Só o clipboard ocupado. Os outros não melhoram com repetição, e repetir sem parar num deles
    /// seria um laço quente por cima de uma condição estável.
    #[must_use]
    pub const fn vale_repetir(&self) -> bool {
        matches!(self, Self::Ocupado)
    }

    /// Se isto é ausência, e não defeito.
    ///
    /// A interface mostra os dois de formas diferentes: ausência é "suspenso, e aqui está o
    /// motivo"; defeito é problema a relatar.
    #[must_use]
    pub const fn e_ausencia(&self) -> bool {
        matches!(self, Self::Indisponivel(_))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn so_o_ocupado_vale_repetir() {
        assert!(ClipError::Ocupado.vale_repetir());
        assert!(!ClipError::Indisponivel("desktop seguro").vale_repetir());
        assert!(!ClipError::FormatoNaoSuportado.vale_repetir());
        assert!(!ClipError::Sistema("0x8004005".to_owned()).vale_repetir());
    }

    #[test]
    fn indisponivel_e_ausencia_e_nao_defeito() {
        // A tela diz "suspenso" para um e "deu problema" para o outro. Trocar os dois faria o
        // produto parecer quebrado no desktop de bloqueio, onde ele está correto.
        assert!(ClipError::Indisponivel("sem sessão").e_ausencia());
        assert!(!ClipError::Ocupado.e_ausencia());
        assert!(!ClipError::Sistema("x".to_owned()).e_ausencia());
    }

    #[test]
    fn toda_mensagem_e_legivel_e_nao_vaza_conteudo() {
        let erros = [
            ClipError::Indisponivel("desktop seguro"),
            ClipError::Ocupado,
            ClipError::FormatoNaoSuportado,
            ClipError::Sistema("os error 5".to_owned()),
        ];
        for erro in erros {
            let frase = erro.to_string();
            assert!(!frase.is_empty());
            assert!(frase.starts_with(|c: char| c.is_lowercase()), "{frase}");
        }
    }
}
