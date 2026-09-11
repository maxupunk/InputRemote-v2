//! Como a interface abre o canal até o serviço — e só isso.
//!
//! Separado da conexão ([`crate::conexao`]) de propósito: abrir um *named pipe* ou um socket Unix é
//! um detalhe de plataforma, e manter a vida da conexão (perceber a queda, esperar, reconectar)
//! dependente só desta abstração é o que permite testar a reconexão inteira sem serviço, sem
//! *pipe* e sem sistema operacional específico — os testes entregam um conector próprio.

use std::io::{Read, Write};

/// As duas metades de um canal duplex: por onde se escreve e por onde se lê.
pub type Duplex = (Box<dyn Write + Send>, Box<dyn Read + Send>);

/// Quem sabe abrir um canal até o serviço.
pub trait Conector: Send {
    /// Abre um canal novo.
    ///
    /// # Errors
    ///
    /// O erro do sistema ao abrir. `NotFound` e afins significam serviço fora do ar;
    /// `PermissionDenied`, que o canal existe e este usuário não o alcança.
    fn abrir(&self) -> std::io::Result<Duplex>;
}

/// O conector desta plataforma: *named pipe* no Windows, socket Unix no Linux.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConectorLocal {
    endereco: String,
}

impl ConectorLocal {
    /// O conector para o endereço padrão, ou para o de `IR_CONTROL_ENDPOINT` se houver.
    #[must_use]
    pub fn padrao() -> Self {
        Self {
            endereco: endereco_configurado(),
        }
    }

    /// Onde este conector procura o serviço.
    #[must_use]
    pub fn endereco(&self) -> &str {
        &self.endereco
    }
}

impl Default for ConectorLocal {
    fn default() -> Self {
        Self::padrao()
    }
}

impl Conector for ConectorLocal {
    fn abrir(&self) -> std::io::Result<Duplex> {
        abrir_canal(&self.endereco)
    }
}

/// O endereço do canal de controle, igual ao do serviço, com o mesmo `IR_CONTROL_ENDPOINT`.
///
/// A sobrescrita aceita caminho completo ou nome curto, **exatamente como no serviço**: um valor
/// sem separador vira `\\.\pipe\<nome>` no Windows e um socket em `TMP` no Linux. Interpretar o
/// mesmo `IR_CONTROL_ENDPOINT` de dois jeitos diferentes faria a interface procurar o serviço num
/// lugar em que ele não está.
fn endereco_configurado() -> String {
    match std::env::var("IR_CONTROL_ENDPOINT") {
        Ok(valor) if !valor.is_empty() => expandir(&valor),
        _ => padrao(),
    }
}

/// Expande uma sobrescrita curta para um endereço completo da plataforma.
fn expandir(valor: &str) -> String {
    if valor.contains(['\\', '/']) {
        return valor.to_owned();
    }
    #[cfg(windows)]
    {
        format!(r"\\.\pipe\{valor}")
    }
    #[cfg(not(windows))]
    {
        std::env::temp_dir()
            .join(format!("{valor}.sock"))
            .to_string_lossy()
            .into_owned()
    }
}

/// O endereço padrão da plataforma.
fn padrao() -> String {
    #[cfg(windows)]
    {
        r"\\.\pipe\inputremote-control".to_owned()
    }
    #[cfg(not(windows))]
    {
        "/run/inputremote/control.sock".to_owned()
    }
}

/// Abre a conexão e devolve as duas metades sobre o mesmo canal duplex.
#[cfg(windows)]
fn abrir_canal(endereco: &str) -> std::io::Result<Duplex> {
    use std::fs::OpenOptions;
    let escrita = OpenOptions::new().read(true).write(true).open(endereco)?;
    let leitura = escrita.try_clone()?;
    Ok((Box::new(escrita), Box::new(leitura)))
}

/// Abre a conexão e devolve as duas metades sobre o mesmo canal duplex.
#[cfg(not(windows))]
fn abrir_canal(endereco: &str) -> std::io::Result<Duplex> {
    use std::os::unix::net::UnixStream;
    let escrita = UnixStream::connect(endereco)?;
    let leitura = escrita.try_clone()?;
    Ok((Box::new(escrita), Box::new(leitura)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn um_caminho_completo_nao_e_mexido() {
        let caminho = if cfg!(windows) {
            r"\\.\pipe\algum-nome"
        } else {
            "/tmp/algum.sock"
        };
        assert_eq!(expandir(caminho), caminho);
    }

    #[test]
    fn um_nome_curto_vira_um_endereco_da_plataforma() {
        let expandido = expandir("ir-teste");
        assert!(expandido.contains("ir-teste"), "{expandido}");
        assert_ne!(expandido, "ir-teste", "nome curto precisa virar endereço");
    }
}
