//! Quem conectou ao serviço, e o que essa pessoa pode.
//!
//! O serviço roda com mais autoridade que quem fala com ele — root no Linux, SYSTEM no Windows — e
//! tudo que ele faz a pedido de alguém passa por duas perguntas: **se** essa pessoa pode falar com
//! ele, e **o que** ela poderia fazer sozinha. As respostas dependem do sistema operacional, e mais
//! que isso não: por isso moram juntas, e fora do serviço.
//!
//! - [`porteiro`] decide quem entra em cada canal, pela credencial e pelos grupos — pura, testável.
//! - [`grupo`], no Linux, consulta a filiação a grupos **na hora**, no banco de usuários.
//! - [`seguranca`], no Windows, escreve em SDDL quem pode abrir cada *pipe*.
//! - [`identidade`], no Windows, identifica o cliente do *pipe* e pergunta ao sistema se ele
//!   poderia ler o que pediu para enviar — a outra metade de `ir_files::permissao`.
//!
//! Saiu do `ir-daemon` quando o serviço passou do teto de 2 500 linhas de produção com a
//! identificação do cliente do *pipe* ([09, §1](../../../docs/09-padroes-de-codigo.md)): o limite
//! apontou uma fronteira que já existia em quatro módulos.

pub mod porteiro;

#[cfg(target_os = "linux")]
pub mod grupo;

#[cfg(windows)]
pub mod identidade;
#[cfg(windows)]
pub mod seguranca;

pub use porteiro::{Chamada, Chamador, decidir};

/// Quem pode abrir um ponto de escuta do serviço.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Acesso {
    /// Só o serviço e os administradores.
    ///
    /// É o do canal do agente, que carrega injeção de entrada: se um processo qualquer do
    /// usuário pudesse abri-lo, qualquer programa que ele rodasse poderia digitar no prompt de
    /// UAC ([04, §5](../../../docs/04-seguranca.md)).
    Restrito,
    /// Também o usuário interativo, que é quem tem a janela na frente.
    UsuarioInterativo,
}

/// Só o serviço (`SY`) e os administradores (`BA`), com o DACL protegido contra herança.
#[cfg(windows)]
const SDDL_RESTRITO: &str = "D:P(A;;GA;;;SY)(A;;GA;;;BA)";
/// O anterior, mais leitura e escrita para o usuário interativo (`IU`).
#[cfg(windows)]
const SDDL_INTERATIVO: &str = "D:P(A;;GA;;;SY)(A;;GA;;;BA)(A;;GRGW;;;IU)";

/// A pasta de estado do serviço: só ele e os administradores, também no que for criado dentro.
///
/// Guarda a chave privada da máquina; herdar a leitura que `%ProgramData%` dá a todos os usuários
/// deixava qualquer conta local se passar por esta máquina diante do par.
#[cfg(windows)]
pub const SDDL_PASTA_DE_ESTADO: &str = "D:P(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)";
/// A pasta de recebidos dentro dela: o usuário interativo lê, move e apaga o que chegou para ele.
#[cfg(windows)]
pub const SDDL_PASTA_DE_RECEBIDOS: &str =
    "D:P(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)(A;OICI;0x1301bf;;;IU)";

#[cfg(windows)]
impl Acesso {
    /// A cadeia SDDL correspondente.
    #[must_use]
    pub const fn sddl(self) -> &'static str {
        match self {
            Self::Restrito => SDDL_RESTRITO,
            Self::UsuarioInterativo => SDDL_INTERATIVO,
        }
    }
}
