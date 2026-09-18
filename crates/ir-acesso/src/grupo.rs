//! A filiação ao grupo `inputremote`, lida no banco de usuários na hora — e não herdada.
//!
//! O canal de controle do Linux já foi guardado pela permissão do arquivo (`0660
//! root:inputremote`), e isso falhou numa máquina real de um jeito que nenhuma instrução
//! resolvia: a permissão do arquivo é conferida com os grupos **do processo** que conecta, e no
//! GNOME a sessão gráfica nasce do `systemd --user`, que sobrevive ao logout e carrega os grupos
//! de quando nasceu. O usuário entrava no grupo, saía, entrava de novo — e a janela continuava de
//! fora até a máquina ser reiniciada.
//!
//! Aqui a pergunta é outra: não "que grupos este processo carrega?", mas "a que grupos este
//! **usuário** pertence agora?". A resposta vem do banco de usuários (`getgrouplist`, que passa
//! pelo NSS e enxerga também usuários de rede), e um `usermod` vale na conexão seguinte.
//!
//! Este módulo só consulta. Quem decide é [`crate::porteiro`].

#![allow(unsafe_code)]

use std::ffi::{CStr, CString};

/// O grupo que o pacote cria e que dá acesso ao canal de controle.
pub const GRUPO: &str = "inputremote";

/// Tamanho do espaço para a entrada do banco de usuários.
///
/// Folgado para nomes, diretórios e shells longos; uma entrada que não caiba simplesmente não é
/// encontrada, e a conexão é negada — nunca permitida por engano.
const ESPACO_DA_ENTRADA: usize = 16 * 1024;

/// Quantas vezes se aumenta a lista de grupos antes de desistir.
const TENTATIVAS_DE_GRUPOS: usize = 4;

/// O usuário efetivo deste processo — o dono do serviço.
pub fn uid_efetivo() -> u32 {
    // SAFETY: `geteuid` não recebe parâmetro, não falha e não tem pré-condição.
    unsafe { libc::geteuid() }
}

/// O identificador numérico de um grupo, pelo nome.
pub fn gid_do_grupo(nome: &str) -> Option<u32> {
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

/// Os grupos a que o usuário `uid` pertence agora, segundo o banco de usuários.
///
/// Vazio se o usuário não for encontrado — o que faz a conexão ser negada, nunca permitida.
pub fn grupos_do_usuario(uid: u32) -> Vec<u32> {
    let Some((nome, gid_primario)) = entrada_do_usuario(uid) else {
        return Vec::new();
    };
    grupos_de(&nome, gid_primario)
}

/// O nome e o grupo primário do usuário, pelo uid.
fn entrada_do_usuario(uid: u32) -> Option<(CString, u32)> {
    let mut espaco: Vec<libc::c_char> = vec![0; ESPACO_DA_ENTRADA];
    // SAFETY: `passwd` é uma estrutura de inteiros e ponteiros; zerada, todos os ponteiros são
    // nulos, o que é um valor válido até `getpwuid_r` preenchê-la.
    let mut entrada: libc::passwd = unsafe { std::mem::zeroed() };
    let mut achada: *mut libc::passwd = std::ptr::null_mut();
    // SAFETY: `entrada` e `achada` são destinos válidos; `espaco` tem o tamanho declarado e vive
    // até o fim desta função, que é até onde os ponteiros de `entrada` são usados.
    let codigo = unsafe {
        libc::getpwuid_r(
            uid,
            std::ptr::from_mut(&mut entrada),
            espaco.as_mut_ptr(),
            espaco.len(),
            std::ptr::from_mut(&mut achada),
        )
    };
    if codigo != 0 || achada.is_null() || entrada.pw_name.is_null() {
        return None;
    }
    // SAFETY: `pw_name` não é nulo e aponta para dentro de `espaco`, que ainda está vivo; a cópia
    // é feita antes de `espaco` sair de escopo.
    let nome = unsafe { CStr::from_ptr(entrada.pw_name) }.to_owned();
    Some((nome, entrada.pw_gid))
}

/// Todos os grupos do usuário, inclusive o primário.
fn grupos_de(nome: &CStr, gid_primario: u32) -> Vec<u32> {
    let mut capacidade: libc::c_int = 64;
    for _ in 0..TENTATIVAS_DE_GRUPOS {
        let tamanho = usize::try_from(capacidade).unwrap_or(0).max(1);
        let mut grupos: Vec<libc::gid_t> = vec![0; tamanho];
        let mut quantos = capacidade;
        // SAFETY: `nome` é terminado em nulo; `grupos` tem espaço para `quantos` itens, e
        // `quantos` é um destino válido que a libc atualiza com o total encontrado.
        let codigo = unsafe {
            libc::getgrouplist(
                nome.as_ptr(),
                gid_primario,
                grupos.as_mut_ptr(),
                std::ptr::from_mut(&mut quantos),
            )
        };
        if codigo >= 0 {
            grupos.truncate(usize::try_from(quantos).unwrap_or(0));
            return grupos;
        }
        // A lista não coube: a libc devolveu em `quantos` quanto precisa.
        capacidade = quantos.max(capacidade.saturating_mul(2));
    }
    Vec::new()
}
