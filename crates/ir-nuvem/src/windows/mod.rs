//! A Cloud Files API, embrulhada.
//!
//! Todo `unsafe` do crate mora aqui e nos submódulos, cada bloco com o porquê de ser seguro. A
//! regra é a de [09, §6](../../../../docs/09-padroes-de-codigo.md): fronteira de sistema, ponteiros
//! que vivem só durante a chamada, nada guardado do outro lado sem dono claro.

#![allow(unsafe_code)]

mod conexao;
mod marcador;
mod registro;

use std::os::windows::ffi::OsStrExt;
use std::path::Path;

pub use conexao::{Conexao, conectar, entregar, falhar};
pub use marcador::{
    atualizar_marcador, criar_marcador, desidratar, hidratar, marcar_em_dia, reverter, situacao,
};
pub use registro::{desregistrar, limpar_orfas, registrar, suportado};

/// Um caminho em UTF-16 terminado em zero, para as funções `W`.
fn largo(caminho: &Path) -> Vec<u16> {
    caminho.as_os_str().encode_wide().chain(Some(0)).collect()
}

/// Um texto em UTF-16 terminado em zero.
fn largo_texto(texto: &str) -> Vec<u16> {
    texto.encode_utf16().chain(Some(0)).collect()
}

/// Nanossegundos desde 1970 em `FILETIME` (centenas de nanossegundos desde 1601).
fn filetime(unix_ns: i64) -> i64 {
    const DE_1601_A_1970: i64 = 116_444_736_000_000_000;
    unix_ns.div_euclid(100).saturating_add(DE_1601_A_1970)
}

/// Um erro do Windows no vocabulário da biblioteca padrão.
fn io(erro: &windows::core::Error) -> std::io::Error {
    std::io::Error::other(erro.message())
}

#[cfg(test)]
mod testes;
