//! O serviço visto pelo sistema operacional: como ele nasce, onde registra, e o que o sistema avisa.
//!
//! Tudo aqui é conversa com o sistema, e nada é decisão do produto:
//!
//! - [`scm`], no Windows: o laço do Gerenciador de Serviços — registrar, responder a "parar", aos
//!   avisos de energia e de sessão, e sair com o código que dispara o reinício automático;
//! - [`registro`]: para onde vão as linhas de registro, sem nunca bloquear quem registra;
//! - [`EventoDoSistema`]: suspender, acordar, trocar de sessão, bloquear — de onde quer que venham;
//! - no Linux, o gancho de suspensão do `systemd` (sinais) e o `logind` (tela bloqueada).
//!
//! Saiu do `ir-daemon` quando o serviço passou do teto de 2 500 linhas de produção com a varredura
//! de melhorias ([09, §1](../../../docs/09-padroes-de-codigo.md)): o que o sistema operacional
//! pede do processo já era uma fronteira — o ator não sabe de SCM, de sinal nem de `logind`, ele só
//! recebe [`EventoDoSistema`].

#[cfg(target_os = "linux")]
pub mod logind;
pub mod registro;
#[cfg(windows)]
pub mod scm;
#[cfg(unix)]
pub mod sinais;

/// Um aviso do sistema operacional, para o ator do serviço.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventoDoSistema {
    /// A máquina vai suspender ou hibernar.
    Suspendendo,
    /// A máquina acordou.
    Retomou,
    /// Alguém entrou, saiu, bloqueou ou trocou de usuário na sessão de console. Só o Windows avisa.
    SessaoMudou,
    /// A tela desta máquina foi bloqueada — o Win+L, e não o UAC, que também usa o desktop seguro.
    TelaBloqueada,
}
