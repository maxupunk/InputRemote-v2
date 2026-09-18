//! Quem está do outro lado do *pipe* — para o serviço, que é SYSTEM, não ler por ninguém o que
//! essa pessoa não leria.
//!
//! # O que se captura, e o que não
//!
//! O token do cliente, em nível de **identificação**: dá para perguntar ao Windows "este usuário
//! poderia ler isto?", e não dá para agir como ele. É o nível com que o `tokio` abre o *pipe* do lado
//! do cliente, e é de propósito que ele não é elevado: com nível de personificação, um programa que
//! registrasse um *pipe* com o nosso nome antes do serviço poderia agir como o usuário que
//! conectasse nele (*pipe squatting*). Para perguntar, identificação basta.
//!
//! # Onde se pergunta
//!
//! No objeto **já aberto**: o descritor de segurança vem do próprio handle (`GetSecurityInfo`), e o
//! `AccessCheck` responde com a ACL inteira — herança, grupos, negações —, que é o que o Windows faria
//! se o próprio usuário abrisse. Reimplementar a regra aqui seria errar em algum desses casos.
//!
//! Ver `ir_files::permissao`, onde a pergunta é feita.
//!
//! [`ir_files::Autorizacao`] é a interface; este módulo, a implementação no Windows.

#![allow(unsafe_code)]

use std::os::windows::io::AsRawHandle;

use windows::Win32::Foundation::{CloseHandle, HANDLE, HLOCAL, LocalFree};
use windows::Win32::Security::Authorization::{GetSecurityInfo, SE_FILE_OBJECT};
use windows::Win32::Security::{
    AccessCheck, DACL_SECURITY_INFORMATION, GENERIC_MAPPING, GROUP_SECURITY_INFORMATION,
    OWNER_SECURITY_INFORMATION, PRIVILEGE_SET, PSECURITY_DESCRIPTOR, RevertToSelf, TOKEN_QUERY,
};
use windows::Win32::Storage::FileSystem::{
    FILE_ALL_ACCESS, FILE_GENERIC_EXECUTE, FILE_GENERIC_READ, FILE_GENERIC_WRITE,
    FILE_LIST_DIRECTORY,
};
use windows::Win32::System::Pipes::ImpersonateNamedPipeClient;
use windows::Win32::System::Threading::{GetCurrentThread, OpenThreadToken};

/// O token de quem conectou, só para perguntar.
#[derive(Debug)]
pub struct TokenDoCliente(HANDLE);

// SAFETY: um token é objeto do núcleo, válido em qualquer thread do processo. O handle é só lido
// (`AccessCheck`), e fechado uma vez, no `Drop`.
unsafe impl Send for TokenDoCliente {}
// SAFETY: como acima; `AccessCheck` pode ser chamado de várias threads com o mesmo token.
unsafe impl Sync for TokenDoCliente {}

impl TokenDoCliente {
    /// O token de quem está do outro lado deste *pipe*.
    ///
    /// Só funciona **depois** de o serviço ter lido algo do *pipe*: antes disso o Windows não
    /// deixa, e é por isso que quem chama lê o primeiro pedido antes de perguntar.
    ///
    /// # Errors
    ///
    /// Erro do sistema se o cliente não puder ser identificado.
    pub fn do_pipe(pipe: &impl AsRawHandle) -> std::io::Result<Self> {
        let cano = HANDLE(pipe.as_raw_handle());
        // SAFETY: `cano` é o handle do servidor do pipe, vivo enquanto `pipe` for emprestado aqui.
        // A thread passa a ter a identidade do cliente até o `RevertToSelf` logo abaixo, sem
        // nenhum `await` no meio: nada mais roda nesta thread com essa identidade.
        unsafe { ImpersonateNamedPipeClient(cano) }.map_err(std::io::Error::from)?;
        let mut token = HANDLE::default();
        // SAFETY: `GetCurrentThread` é um pseudo-handle sempre válido. `openasself` faz a abertura
        // ser conferida com a identidade do serviço, e não com a do cliente, que pode não ter
        // direito sobre a própria thread do serviço.
        let aberto =
            unsafe { OpenThreadToken(GetCurrentThread(), TOKEN_QUERY, true, &raw mut token) };
        // SAFETY: sem argumentos; devolve a thread à identidade do processo.
        if unsafe { RevertToSelf() }.is_err() {
            // Uma thread do serviço presa na identidade de outro usuário é um estado que não se
            // deixa continuar: o que ela fizesse depois teria a autoridade errada.
            std::process::abort();
        }
        aberto.map_err(std::io::Error::from)?;
        Ok(Self(token))
    }
}

impl ir_files::Autorizacao for TokenDoCliente {
    fn pode_ler(&self, aberto: &std::fs::File, pasta: bool) -> std::io::Result<bool> {
        let descritor = Descritor::do_arquivo(aberto)?;
        let mapa = GENERIC_MAPPING {
            GenericRead: FILE_GENERIC_READ.0,
            GenericWrite: FILE_GENERIC_WRITE.0,
            GenericExecute: FILE_GENERIC_EXECUTE.0,
            GenericAll: FILE_ALL_ACCESS.0,
        };
        let pedido = if pasta {
            FILE_LIST_DIRECTORY.0
        } else {
            FILE_GENERIC_READ.0
        };
        let mut privilegios = PRIVILEGE_SET::default();
        let mut tamanho = u32::try_from(size_of::<PRIVILEGE_SET>()).unwrap_or(0);
        let mut concedido = 0u32;
        let mut permitido = windows::core::BOOL(0);
        // SAFETY: o descritor é válido enquanto `descritor` viver (até o fim desta função); o
        // token, enquanto `self` viver. Os ponteiros de saída apontam para variáveis locais.
        unsafe {
            AccessCheck(
                descritor.0,
                self.0,
                pedido,
                &raw const mapa,
                Some(&raw mut privilegios),
                &raw mut tamanho,
                &raw mut concedido,
                &raw mut permitido,
            )
        }
        .map_err(std::io::Error::from)?;
        Ok(permitido.as_bool())
    }
}

impl Drop for TokenDoCliente {
    fn drop(&mut self) {
        // SAFETY: o handle foi aberto por `OpenThreadToken` e só é fechado aqui.
        let _ = unsafe { CloseHandle(self.0) };
    }
}

/// O descritor de segurança de um objeto aberto, devolvido ao Windows ao sair.
struct Descritor(PSECURITY_DESCRIPTOR);

impl Descritor {
    /// Dono, grupo e DACL: o `AccessCheck` recusa um descritor sem dono e grupo.
    fn do_arquivo(aberto: &std::fs::File) -> std::io::Result<Self> {
        let mut descritor = PSECURITY_DESCRIPTOR::default();
        // SAFETY: o handle do arquivo é válido enquanto `aberto` for emprestado aqui, e foi aberto
        // com leitura, que inclui `READ_CONTROL`. O descritor devolvido é liberado no `Drop`.
        unsafe {
            GetSecurityInfo(
                HANDLE(aberto.as_raw_handle()),
                SE_FILE_OBJECT,
                OWNER_SECURITY_INFORMATION | GROUP_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
                None,
                None,
                None,
                None,
                Some(&raw mut descritor),
            )
        }
        .ok()
        .map_err(std::io::Error::from)?;
        Ok(Self(descritor))
    }
}

impl Drop for Descritor {
    fn drop(&mut self) {
        // SAFETY: memória alocada pelo `GetSecurityInfo`, que pede `LocalFree`; liberada uma vez.
        let _ = unsafe { LocalFree(Some(HLOCAL(self.0.0))) };
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use ir_files::Autorizacao;

    use super::*;

    /// O token deste próprio processo, como se fosse o de um cliente.
    fn este_processo() -> TokenDoCliente {
        use windows::Win32::Security::{DuplicateToken, SecurityIdentification, TOKEN_DUPLICATE};
        use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
        let mut primario = HANDLE::default();
        let mut identificacao = HANDLE::default();
        // SAFETY: pseudo-handle do processo; os handles de saída são locais e fechados abaixo.
        unsafe {
            OpenProcessToken(GetCurrentProcess(), TOKEN_DUPLICATE, &raw mut primario).unwrap();
            DuplicateToken(primario, SecurityIdentification, &raw mut identificacao).unwrap();
            let _ = CloseHandle(primario);
        }
        TokenDoCliente(identificacao)
    }

    #[test]
    fn o_que_eu_crio_eu_posso_ler() {
        let dir = std::env::temp_dir().join(format!("ir-identidade-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let caminho = dir.join("meu.txt");
        std::fs::write(&caminho, b"meu").unwrap();
        let token = este_processo();
        let arquivo = std::fs::File::open(&caminho).unwrap();
        assert!(token.pode_ler(&arquivo, false).unwrap());
        let pasta = {
            use std::os::windows::fs::OpenOptionsExt;
            std::fs::OpenOptions::new()
                .read(true)
                .custom_flags(windows::Win32::Storage::FileSystem::FILE_FLAG_BACKUP_SEMANTICS.0)
                .open(&dir)
                .unwrap()
        };
        assert!(token.pode_ler(&pasta, true).unwrap());
        drop((arquivo, pasta));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn uma_acl_que_so_deixa_ver_a_acl_nao_deixa_ler() {
        use std::os::windows::fs::OpenOptionsExt;
        use windows::Win32::Security::SetFileSecurityW;
        use windows::Win32::Storage::FileSystem::READ_CONTROL;
        use windows::core::HSTRING;

        let caminho = std::env::temp_dir().join(format!("ir-negado-{}.txt", std::process::id()));
        std::fs::write(&caminho, b"segredo").unwrap();
        let nome = HSTRING::from(caminho.as_os_str());
        let aplicar = |sddl: &str| {
            let descritor = crate::seguranca::Descritor::de_sddl(sddl).unwrap();
            // SAFETY: nome e descritor vivos durante a chamada; o dono sempre pode trocar a DACL.
            unsafe { SetFileSecurityW(&nome, DACL_SECURITY_INFORMATION, descritor.bruto()) }
                .unwrap();
        };
        // Só `READ_CONTROL` para todos — mais `SYNCHRONIZE` e `FILE_READ_ATTRIBUTES`, que o
        // `CreateFile` pede sempre: dá para abrir e ver a ACL, e não dá para ler o conteúdo.
        aplicar("D:P(A;;0x120080;;;WD)");
        let aberto = std::fs::OpenOptions::new()
            .access_mode(READ_CONTROL.0)
            .open(&caminho)
            .unwrap();
        let pode = este_processo().pode_ler(&aberto, false);
        drop(aberto);
        aplicar("D:P(A;;FA;;;WD)");
        let _ = std::fs::remove_file(&caminho);
        assert!(!pode.unwrap(), "o AccessCheck deixou ler o que a ACL nega");
    }

    #[tokio::test]
    async fn o_cliente_do_pipe_e_identificado_depois_da_primeira_leitura() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        use tokio::net::windows::named_pipe::{ClientOptions, ServerOptions};

        let nome = format!(r"\\.\pipe\ir-identidade-{}", std::process::id());
        let mut servidor = ServerOptions::new().create(&nome).unwrap();
        // As opções padrão do `tokio`: nível de identificação, o mesmo que o `ir_ipc::cliente` usa.
        let mut cliente = ClientOptions::new().open(&nome).unwrap();
        servidor.connect().await.unwrap();
        cliente.write_all(b"x").await.unwrap();
        let mut um = [0u8; 1];
        servidor.read_exact(&mut um).await.unwrap();

        let token = TokenDoCliente::do_pipe(&servidor).expect("o cliente não foi identificado");
        // O cliente é este mesmo processo: o que ele cria, ele lê.
        let caminho = std::env::temp_dir().join(format!("ir-pipe-{}.txt", std::process::id()));
        std::fs::write(&caminho, b"meu").unwrap();
        let arquivo = std::fs::File::open(&caminho).unwrap();
        assert!(token.pode_ler(&arquivo, false).unwrap());
        drop(arquivo);
        let _ = std::fs::remove_file(&caminho);
    }
}
