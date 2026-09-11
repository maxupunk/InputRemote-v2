//! O grupo que alcança o canal de controle no Linux.
//!
//! Mesma questão que o descritor de segurança resolve no Windows, e pelo mesmo motivo: o serviço
//! roda privilegiado, então o socket nasce `root:root`, e a janela do usuário — que roda sem
//! privilégio nenhum — leva "permissão negada" e cai para o modo de demonstração.
//!
//! A saída **não** é abrir o socket para todo mundo. É a de
//! [02, §7](../../../docs/02-arquitetura.md): o canal de controle fica `0660 root:inputremote`, e
//! quem for operar a máquina entra nesse grupo. Assim o acesso é uma decisão registrada do
//! administrador, e não um efeito colateral de permissão frouxa.

#![allow(unsafe_code)]

use std::ffi::CString;

use tracing::{info, warn};

/// O grupo que o pacote cria e que dá acesso ao canal de controle.
pub(crate) const GRUPO: &str = "inputremote";

/// Entrega o socket ao grupo [`GRUPO`], se ele existir.
///
/// Não falha o serviço se não der: sem o grupo, tudo continua funcionando pelo terminal, e só a
/// janela fica de fora — o que é registrado com o que fazer a respeito.
pub(crate) fn dar_ao_grupo(caminho: &str) {
    let Some(gid) = gid_do_grupo(GRUPO) else {
        warn!(
            grupo = GRUPO,
            "o grupo não existe, então a janela do usuário não vai alcançar o serviço; \
             crie-o com `groupadd -r inputremote` e entre nele com `usermod -aG inputremote <você>`"
        );
        return;
    };
    let Ok(caminho_c) = CString::new(caminho.as_bytes()) else {
        return;
    };
    // SAFETY: o caminho é um ponteiro válido terminado em nulo, vivo até o fim da chamada.
    // `u32::MAX` é o `-1` de `uid_t`, que manda preservar o dono e trocar só o grupo.
    let resultado = unsafe { libc::chown(caminho_c.as_ptr(), u32::MAX, gid) };
    if resultado == 0 {
        info!(grupo = GRUPO, "canal de controle entregue ao grupo");
    } else {
        warn!(grupo = GRUPO, "não foi possível entregar o canal ao grupo");
    }
}

/// O identificador numérico de um grupo, pelo nome.
fn gid_do_grupo(nome: &str) -> Option<u32> {
    let nome_c = CString::new(nome).ok()?;
    // SAFETY: ponteiro válido terminado em nulo. O retorno aponta para memória estática da libc,
    // lida imediatamente e não guardada.
    let entrada = unsafe { libc::getgrnam(nome_c.as_ptr()) };
    if entrada.is_null() {
        return None;
    }
    // SAFETY: `entrada` não é nulo, e aponta para uma `group` válida preenchida pela libc.
    Some(unsafe { (*entrada).gr_gid })
}
