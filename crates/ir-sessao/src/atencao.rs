//! Ctrl+Alt+Del gerado por software: a Sequência de Atenção Segura.
//!
//! `SendInput` não gera Ctrl+Alt+Del — por projeto do Windows. O caminho é `SendSAS(FALSE)`, de
//! `sas.dll`, chamado **pelo serviço**, e ele só funciona com a política
//! `SoftwareSASGeneration` ligando os serviços ([05, §4.3](../../../docs/05-windows.md)).
//!
//! A política é da máquina, e o produto só a toca quando um administrador liga a digitação do par
//! na tela de bloqueio — é o mesmo consentimento, com a mesma consequência. Desligar a permissão
//! devolve o valor que estava lá antes, se foi o produto quem o mudou.

#![allow(unsafe_code)]

use anyhow::{Context, Result, bail};

/// Onde mora a política.
const CHAVE: &str = r"HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\Policies\System";
/// O nome do valor.
const VALOR: &str = "SoftwareSASGeneration";

/// Quem pode gerar Ctrl+Alt+Del por software, pela política do sistema.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PoliticaDeAtencao {
    /// Ninguém (o padrão do Windows).
    Ninguem,
    /// Os serviços — o que o produto precisa.
    Servicos,
    /// Os aplicativos de acessibilidade.
    Acessibilidade,
    /// Os dois.
    Ambos,
}

impl PoliticaDeAtencao {
    /// Se os serviços podem gerar a sequência.
    #[must_use]
    pub const fn permite_servicos(self) -> bool {
        matches!(self, Self::Servicos | Self::Ambos)
    }

    /// A política pelo número que o registro guarda.
    #[must_use]
    pub const fn do_numero(numero: u32) -> Self {
        match numero {
            1 => Self::Servicos,
            2 => Self::Acessibilidade,
            3 => Self::Ambos,
            _ => Self::Ninguem,
        }
    }

    /// O número que o registro guarda.
    #[must_use]
    pub const fn numero(self) -> u32 {
        match self {
            Self::Ninguem => 0,
            Self::Servicos => 1,
            Self::Acessibilidade => 2,
            Self::Ambos => 3,
        }
    }

    /// A política que permite os serviços sem tirar o que já estava permitido.
    #[must_use]
    pub const fn com_servicos(self) -> Self {
        match self {
            Self::Ninguem | Self::Servicos => Self::Servicos,
            Self::Acessibilidade | Self::Ambos => Self::Ambos,
        }
    }
}

/// Lê a política, pelo `reg.exe` — o valor ausente é "ninguém".
#[must_use]
pub fn politica() -> PoliticaDeAtencao {
    let Ok(saida) = std::process::Command::new("reg")
        .args(["query", CHAVE, "/v", VALOR])
        .output()
    else {
        return PoliticaDeAtencao::Ninguem;
    };
    let texto = String::from_utf8_lossy(&saida.stdout);
    PoliticaDeAtencao::do_numero(ler_dword(&texto).unwrap_or(0))
}

/// Grava a política.
///
/// # Errors
///
/// Se o `reg.exe` recusar — sem privilégio de administrador, por exemplo.
pub fn gravar_politica(politica: PoliticaDeAtencao) -> Result<()> {
    let numero = politica.numero().to_string();
    let saida = std::process::Command::new("reg")
        .args([
            "add",
            CHAVE,
            "/v",
            VALOR,
            "/t",
            "REG_DWORD",
            "/d",
            &numero,
            "/f",
        ])
        .output()
        .context("rodando reg.exe")?;
    if !saida.status.success() {
        bail!(
            "reg.exe recusou: {}",
            String::from_utf8_lossy(&saida.stderr).trim()
        );
    }
    Ok(())
}

/// O número de uma linha `SoftwareSASGeneration    REG_DWORD    0x1` do `reg query`.
///
/// Lê o hexadecimal, e não o texto em volta: o `reg.exe` muda de idioma com o sistema.
fn ler_dword(saida: &str) -> Option<u32> {
    saida
        .lines()
        .find(|linha| linha.contains(VALOR))?
        .split_whitespace()
        .find_map(|parte| parte.strip_prefix("0x"))
        .and_then(|hex| u32::from_str_radix(hex, 16).ok())
}

/// Gera Ctrl+Alt+Del na sessão de console.
///
/// Só funciona chamado por um serviço como `LocalSystem`, com a política permitindo os serviços.
/// Sem a política, o Windows simplesmente não faz nada — por isso quem chama confere antes.
pub fn enviar_sas() {
    // SAFETY: a função não recebe ponteiro e não tem pré-condição além do contexto, conferido por
    // quem chama; no contexto errado ela não faz nada.
    unsafe { windows::Win32::Security::Authentication::Identity::SendSAS(false) };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_leitura_vale_em_qualquer_idioma() {
        let saida = "\r\nHKEY_LOCAL_MACHINE\\SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Policies\\System\r\n    SoftwareSASGeneration    REG_DWORD    0x3\r\n";
        assert_eq!(ler_dword(saida), Some(3));
        assert_eq!(ler_dword("nada aqui"), None);
    }

    #[test]
    fn permitir_os_servicos_nao_tira_a_acessibilidade() {
        assert_eq!(
            PoliticaDeAtencao::Acessibilidade.com_servicos(),
            PoliticaDeAtencao::Ambos
        );
        assert_eq!(
            PoliticaDeAtencao::Ninguem.com_servicos(),
            PoliticaDeAtencao::Servicos
        );
        assert!(PoliticaDeAtencao::Ambos.permite_servicos());
        assert!(!PoliticaDeAtencao::Acessibilidade.permite_servicos());
    }

    #[test]
    fn a_politica_desta_maquina_e_legivel() {
        // Sem administrador não se grava, mas ler sempre se lê.
        let _ = politica();
    }
}
