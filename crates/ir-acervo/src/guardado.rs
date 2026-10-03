//! O índice de cada pasta, guardado em disco.
//!
//! Um arquivo por pasta, em `postcard`, com um cabeçalho que diz o formato: um índice de uma versão
//! que este código não lê é deixado de lado com aviso, e não lido torto. A gravação é atômica —
//! arquivo ao lado, `fsync`, renomear —, para uma queda de energia no meio deixar o índice de antes,
//! e nunca meio índice.
//!
//! Perder o índice não perde arquivo: a pasta é varrida de novo, e o que diferir vira cópia de
//! conflito, nunca remoção.

use std::io::Write;
use std::path::{Path, PathBuf};

use ir_pasta::{Origem, Replica};
use ir_proto::message::FolderId;
use serde::{Deserialize, Serialize};
use tracing::warn;

/// Os quatro primeiros bytes de todo índice.
const MAGICO: [u8; 4] = *b"IRPI";

/// O formato do índice. Sobe quando um tipo guardado muda.
const FORMATO: u16 = 2;

/// O nome do arquivo do índice, dentro da pasta de estado da pasta compartilhada.
const ARQUIVO: &str = "indice.bin";

/// O índice, conforme o papel deste computador na pasta.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Indice {
    /// Este computador compartilhou.
    Origem(Origem),
    /// Este computador recebeu.
    Replica(Replica),
}

/// Tudo o que se guarda de uma pasta.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Guardada {
    /// O nome, como aparece nos dois computadores.
    pub nome: String,
    /// Onde o conteúdo dela fica neste computador — a pasta que a sincronia varre.
    pub raiz: PathBuf,
    /// Onde a pessoa a vê, quando não é a raiz: a pasta sob demanda do Linux é um sistema de
    /// arquivos montado aqui, mostrando o conteúdo que fica na raiz, escondida.
    pub ponto: Option<PathBuf>,
    /// O índice.
    pub indice: Indice,
    /// Na origem: se o outro lado já aceitou. Na réplica é sempre verdadeiro.
    pub aceita: bool,
    /// Os conflitos que o usuário ainda não olhou: o caminho original e o da cópia.
    pub conflitos: Vec<(String, String)>,
}

impl Guardada {
    /// A pasta, pelo índice.
    #[must_use]
    pub const fn pasta(&self) -> FolderId {
        match &self.indice {
            Indice::Origem(origem) => origem.pasta(),
            Indice::Replica(replica) => replica.pasta(),
        }
    }
}

/// Grava o índice de uma pasta em `dir`.
///
/// # Errors
///
/// Erro de disco ou de codificação.
pub fn gravar(dir: &Path, guardada: &Guardada) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let corpo = postcard::to_allocvec(guardada).map_err(std::io::Error::other)?;
    let temporario = dir.join(format!("{ARQUIVO}.novo"));
    {
        let mut arquivo = std::fs::File::create(&temporario)?;
        arquivo.write_all(&MAGICO)?;
        arquivo.write_all(&FORMATO.to_le_bytes())?;
        arquivo.write_all(&corpo)?;
        arquivo.sync_all()?;
    }
    std::fs::rename(&temporario, dir.join(ARQUIVO))
}

/// Lê o índice guardado em `dir`, se houver um que este código entenda.
#[must_use]
pub fn ler(dir: &Path) -> Option<Guardada> {
    let bytes = std::fs::read(dir.join(ARQUIVO)).ok()?;
    let corpo = bytes
        .strip_prefix(&MAGICO)
        .and_then(|resto| resto.strip_prefix(&FORMATO.to_le_bytes()));
    let Some(corpo) = corpo else {
        warn!(dir = %dir.display(), "índice de outro formato; a pasta fica de fora até ser compartilhada de novo");
        return None;
    };
    match postcard::from_bytes(corpo) {
        Ok(guardada) => Some(guardada),
        Err(erro) => {
            warn!(%erro, "índice ilegível; a pasta fica de fora até ser compartilhada de novo");
            None
        }
    }
}

/// Todas as pastas guardadas sob `estado`.
#[must_use]
pub fn todas(estado: &Path) -> Vec<Guardada> {
    let Ok(entradas) = std::fs::read_dir(estado) else {
        return Vec::new();
    };
    entradas
        .filter_map(Result::ok)
        .filter_map(|entrada| ler(&entrada.path()))
        .collect()
}

/// Esquece uma pasta: o índice sai. A lixeira da réplica, que mora ao lado, vai junto.
pub fn esquecer(dir: &Path) {
    if let Err(erro) = std::fs::remove_dir_all(dir)
        && erro.kind() != std::io::ErrorKind::NotFound
    {
        warn!(%erro, "não consegui apagar o índice de uma pasta esquecida");
    }
}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn o_indice_volta_igual_e_um_formato_estranho_e_recusado() {
        let dir = std::env::temp_dir().join(format!("ir-guardado-{}", std::process::id()));
        let guardada = Guardada {
            nome: "Projetos".into(),
            raiz: PathBuf::from("/tmp/Projetos"),
            ponto: None,
            indice: Indice::Replica(Replica::nova(FolderId([3; 16]), false)),
            aceita: true,
            conflitos: vec![("a.txt".into(), "a (conflito X).txt".into())],
        };
        gravar(&dir, &guardada).unwrap();
        assert_eq!(ler(&dir), Some(guardada));
        std::fs::write(dir.join(ARQUIVO), b"XXXX\x01\x00lixo").unwrap();
        assert_eq!(ler(&dir), None);
        esquecer(&dir);
        assert!(!dir.exists());
    }
}
