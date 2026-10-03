//! Onde as coisas da pasta compartilhada moram neste computador, e quem ele é.
//!
//! | O quê | Windows | Linux |
//! |---|---|---|
//! | Pastas recebidas e criadas | `%USERPROFILE%\InputRemote` | `~/InputRemote` |
//! | Índice, fila e lixeira da réplica | `%LOCALAPPDATA%\InputRemote\pastas` | `~/.local/state/inputremote/pastas` |
//!
//! O índice fica fora da pasta compartilhada de propósito: dentro dela, ele seria sincronizado, e
//! numa raiz sob demanda do Windows ele viraria um arquivo sob demanda também.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use ir_proto::message::FolderId;

/// Os lugares e a identidade deste computador.
#[derive(Debug, Clone)]
pub struct Lugar {
    /// Onde guardar o índice de cada pasta.
    pub estado: PathBuf,
    /// Onde as pastas recebidas e as criadas ficam.
    pub raiz_padrao: PathBuf,
    /// O nome deste computador, para a cópia de conflito.
    pub maquina: String,
    /// O fuso deste computador, em segundos a leste de UTC.
    pub fuso_s: i32,
}

impl Lugar {
    /// Os lugares do usuário que roda este processo.
    ///
    /// # Errors
    ///
    /// Quando o sistema não diz onde fica a pasta do usuário.
    pub fn deste_usuario() -> Result<Self> {
        let (estado, raiz_padrao) = pastas_do_usuario()?;
        Ok(Self {
            estado,
            raiz_padrao,
            maquina: nome_da_maquina(),
            fuso_s: fuso_local(),
        })
    }

    /// Onde fica o que é desta pasta fora dela: o índice e, na réplica, a lixeira.
    #[must_use]
    pub fn da_pasta(&self, pasta: FolderId) -> PathBuf {
        self.estado.join(hex(&pasta.0))
    }
}

/// O identificador da pasta em hexadecimal, para nome de arquivo.
#[must_use]
pub fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    bytes.iter().fold(String::new(), |mut texto, byte| {
        let _ = write!(texto, "{byte:02x}");
        texto
    })
}

#[cfg(windows)]
fn pastas_do_usuario() -> Result<(PathBuf, PathBuf)> {
    let local = std::env::var_os("LOCALAPPDATA").context("LOCALAPPDATA não definido")?;
    let perfil = std::env::var_os("USERPROFILE").context("USERPROFILE não definido")?;
    Ok((
        Path::new(&local).join("InputRemote").join("pastas"),
        Path::new(&perfil).join("InputRemote"),
    ))
}

#[cfg(not(windows))]
fn pastas_do_usuario() -> Result<(PathBuf, PathBuf)> {
    let casa = std::env::var_os("HOME").context("HOME não definido")?;
    let casa = Path::new(&casa);
    let estado = std::env::var_os("XDG_STATE_HOME")
        .filter(|valor| !valor.is_empty())
        .map_or_else(|| casa.join(".local").join("state"), PathBuf::from);
    Ok((
        estado.join("inputremote").join("pastas"),
        casa.join("InputRemote"),
    ))
}

/// O nome deste computador, como o sistema o chama.
fn nome_da_maquina() -> String {
    #[cfg(windows)]
    let nome = std::env::var("COMPUTERNAME").ok();
    #[cfg(not(windows))]
    let nome = std::fs::read_to_string("/etc/hostname")
        .ok()
        .or_else(|| std::env::var("HOSTNAME").ok());
    nome.map(|n| n.trim().to_owned())
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| "este computador".to_owned())
}

/// O fuso local, em segundos a leste de UTC.
///
/// A biblioteca padrão não sabe a hora local. No Linux, `date +%z` responde com o fuso que o
/// sistema usa — horário de verão incluso —, sem ler a base de fusos à mão; no Windows, a API do
/// próprio sistema.
#[cfg(not(windows))]
fn fuso_local() -> i32 {
    let saida = std::process::Command::new("date").arg("+%z").output();
    saida
        .ok()
        .and_then(|saida| String::from_utf8(saida.stdout).ok())
        .and_then(|texto| ler_fuso(texto.trim()))
        .unwrap_or(0)
}

#[cfg(windows)]
#[allow(unsafe_code)]
fn fuso_local() -> i32 {
    use windows::Win32::System::Time::{GetTimeZoneInformation, TIME_ZONE_INFORMATION};
    /// `TIME_ZONE_ID_DAYLIGHT`, de `winnt.h`: o horário de verão está valendo agora.
    const HORARIO_DE_VERAO: u32 = 2;
    let mut info = TIME_ZONE_INFORMATION::default();
    // SAFETY: a estrutura é local e o Windows só escreve nela.
    let qual = unsafe { GetTimeZoneInformation(&raw mut info) };
    // O `Bias` é em minutos a **oeste** de UTC; no horário de verão soma-se o `DaylightBias`.
    let mut minutos = info.Bias;
    if qual == HORARIO_DE_VERAO {
        minutos += info.DaylightBias;
    }
    minutos.saturating_mul(-60)
}

/// `-0300` em segundos: `-10800`.
#[cfg_attr(windows, allow(dead_code))]
fn ler_fuso(texto: &str) -> Option<i32> {
    let (sinal, resto) = match texto.as_bytes().first()? {
        b'-' => (-1, texto.get(1..)?),
        b'+' => (1, texto.get(1..)?),
        _ => (1, texto),
    };
    let horas: i32 = resto.get(..2)?.parse().ok()?;
    let minutos: i32 = resto.get(2..4)?.parse().ok()?;
    Some(sinal * (horas * 3_600 + minutos * 60))
}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn o_fuso_do_date_vira_segundos() {
        assert_eq!(ler_fuso("-0300"), Some(-10_800));
        assert_eq!(ler_fuso("+0530"), Some(19_800));
        assert_eq!(ler_fuso("+0000"), Some(0));
        assert_eq!(ler_fuso("lixo"), None);
    }

    #[test]
    fn o_identificador_vira_nome_de_arquivo() {
        assert_eq!(hex(&[0x01, 0xab, 0xff]), "01abff");
    }
}
