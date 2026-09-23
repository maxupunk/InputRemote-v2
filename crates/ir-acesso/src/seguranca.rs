//! Quem pode falar com o serviço, escrito como descritor de segurança.
//!
//! Este módulo existe por causa de um defeito concreto: um *named pipe* criado por um processo
//! `LocalSystem` com o descritor **padrão** não dá acesso ao usuário interativo. O serviço subia,
//! o canal existia, e a janela levava "acesso negado" ao abrir — caindo para o modo de
//! demonstração sem que nada no sistema parecesse errado.
//!
//! A correção não é afrouxar tudo: são **dois** canais, com dois descritores diferentes
//! ([04, §5](../../../docs/04-seguranca.md)).
//!
//! - O de **controle** aceita o usuário interativo: é a janela dele, e ela precisa perguntar o
//!   estado e conduzir o pareamento.
//! - O do **agente** não aceita ninguém além do serviço e dos administradores. Ele carrega
//!   injeção de entrada; se um processo qualquer do usuário pudesse abri-lo, qualquer programa
//!   que ele rodasse poderia digitar no prompt de UAC.

#![allow(unsafe_code)]

use anyhow::{Context, Result};
use windows::Win32::Foundation::{HLOCAL, LocalFree};
use windows::Win32::Security::Authorization::{
    ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
};
use windows::Win32::Security::{PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES};
use windows::core::{HSTRING, PCWSTR};

/// Um descritor de segurança vivo, pronto para ser passado à criação do *pipe*.
///
/// Guarda a memória que o Windows alocou e a devolve ao ser descartado. Precisa continuar vivo
/// enquanto o ponto de escuta existir: cada instância nova do *pipe* é criada com ele de novo.
pub struct Descritor {
    descritor: PSECURITY_DESCRIPTOR,
    atributos: SECURITY_ATTRIBUTES,
}

// SAFETY: o descritor é um bloco de memória próprio, criado aqui e só lido pelo Windows na
// criação do pipe. Não há referência a estado de thread nem aliasing compartilhado.
unsafe impl Send for Descritor {}

impl core::fmt::Debug for Descritor {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("Descritor")
    }
}

impl Descritor {
    /// Monta um descritor a partir de uma cadeia SDDL.
    ///
    /// # Errors
    ///
    /// Erro do Windows se o SDDL for inválido — o que é defeito de programação, não de ambiente.
    pub fn de_sddl(sddl: &str) -> Result<Self> {
        let texto = HSTRING::from(sddl);
        let mut descritor = PSECURITY_DESCRIPTOR::default();
        // SAFETY: `texto` vive até o fim da chamada e é terminado em nulo; `descritor` é um
        // destino válido. A memória devolvida é liberada em `Drop`.
        unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                PCWSTR(texto.as_ptr()),
                SDDL_REVISION_1,
                std::ptr::from_mut(&mut descritor),
                None,
            )
        }
        .with_context(|| format!("interpretando o SDDL `{sddl}`"))?;

        let atributos = SECURITY_ATTRIBUTES {
            nLength: u32::try_from(size_of::<SECURITY_ATTRIBUTES>()).unwrap_or(0),
            lpSecurityDescriptor: descritor.0,
            bInheritHandle: false.into(),
        };
        Ok(Self {
            descritor,
            atributos,
        })
    }

    /// O descritor em si, para um teste aplicá-lo a um arquivo.
    #[cfg(test)]
    pub(crate) const fn bruto(&self) -> PSECURITY_DESCRIPTOR {
        self.descritor
    }

    /// Cria uma instância do *pipe* protegida por este descritor.
    ///
    /// A criação mora aqui, e não em o ponto de escuta do serviço, para o `unsafe` ficar confinado ao módulo
    /// que já o declara ([09, §4](../../../docs/09-padroes-de-codigo.md)). Quem chama recebe uma
    /// função comum.
    ///
    /// # Errors
    ///
    /// Erro do sistema se o nome já estiver em uso ou o processo não puder criar o *pipe*.
    pub fn criar_pipe(
        &mut self,
        nome: &str,
        primeira: bool,
    ) -> std::io::Result<tokio::net::windows::named_pipe::NamedPipeServer> {
        use tokio::net::windows::named_pipe::ServerOptions;

        let mut opcoes = ServerOptions::new();
        opcoes.first_pipe_instance(primeira);
        let atributos = std::ptr::from_mut(&mut self.atributos).cast();
        // SAFETY: `atributos` aponta para a `SECURITY_ATTRIBUTES` deste descritor, que vive
        // enquanto `self` viver, e o Windows só a lê durante esta chamada.
        unsafe { opcoes.create_with_security_attributes_raw(nome, atributos) }
    }
}

impl Drop for Descritor {
    fn drop(&mut self) {
        // SAFETY: a memória veio de `ConvertStringSecurityDescriptorToSecurityDescriptorW`, que
        // a documentação manda liberar com `LocalFree`, e não é mais usada.
        unsafe {
            let _ = LocalFree(Some(HLOCAL(self.descritor.0)));
        }
    }
}

/// Troca o DACL de uma pasta por um protegido, e o propaga ao que já está dentro dela.
///
/// Existe pela chave da máquina: `%ProgramData%` dá leitura a todos os usuários, e a pasta de
/// estado herdava isso — qualquer conta local lia o `identity.key` e podia se passar por esta
/// máquina diante do par. Protegido (`P`), o DACL deixa de herdar do pai; e
/// `SetNamedSecurityInfoW`, ao contrário de `SetFileSecurityW`, reaplica a herança aos arquivos
/// que já existem, então a chave gravada antes desta correção também é fechada.
///
/// # Errors
///
/// Erro do Windows se o SDDL for inválido ou a pasta não puder ter a segurança trocada.
pub fn proteger_pasta(pasta: &std::path::Path, sddl: &str) -> Result<()> {
    use windows::Win32::Security::Authorization::{SE_FILE_OBJECT, SetNamedSecurityInfoW};
    use windows::Win32::Security::{
        ACL, DACL_SECURITY_INFORMATION, GetSecurityDescriptorDacl,
        PROTECTED_DACL_SECURITY_INFORMATION,
    };

    let descritor = Descritor::de_sddl(sddl)?;
    let mut presente = windows::core::BOOL::default();
    let mut padrao = windows::core::BOOL::default();
    let mut dacl: *mut ACL = std::ptr::null_mut();
    // SAFETY: o descritor é válido enquanto `descritor` viver, e os três destinos são locais.
    unsafe {
        GetSecurityDescriptorDacl(
            descritor.descritor,
            std::ptr::from_mut(&mut presente),
            std::ptr::from_mut(&mut dacl),
            std::ptr::from_mut(&mut padrao),
        )
    }
    .context("lendo o DACL do SDDL")?;
    anyhow::ensure!(presente.as_bool() && !dacl.is_null(), "o SDDL não tem DACL");
    let nome = HSTRING::from(pasta.as_os_str());
    // SAFETY: `nome` termina em nulo e vive até o fim da chamada; `dacl` aponta para dentro de
    // `descritor`, que também vive até lá.
    let erro = unsafe {
        SetNamedSecurityInfoW(
            PCWSTR(nome.as_ptr()),
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
            None,
            None,
            Some(dacl.cast_const()),
            None,
        )
    };
    erro.ok()
        .with_context(|| format!("protegendo {}", pasta.display()))
}

/// O DACL atual de um caminho, em SDDL — para conferir o que [`proteger_pasta`] fez.
///
/// # Errors
///
/// Erro do Windows se o caminho não puder ser lido.
pub fn dacl_em_sddl(caminho: &std::path::Path) -> Result<String> {
    use windows::Win32::Security::Authorization::{
        ConvertSecurityDescriptorToStringSecurityDescriptorW, GetNamedSecurityInfoW, SE_FILE_OBJECT,
    };
    use windows::Win32::Security::DACL_SECURITY_INFORMATION;

    let nome = HSTRING::from(caminho.as_os_str());
    let mut descritor = PSECURITY_DESCRIPTOR::default();
    // SAFETY: `nome` vive até o fim da chamada; o descritor devolvido é liberado abaixo.
    unsafe {
        GetNamedSecurityInfoW(
            PCWSTR(nome.as_ptr()),
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION,
            None,
            None,
            None,
            None,
            std::ptr::from_mut(&mut descritor),
        )
    }
    .ok()
    .with_context(|| format!("lendo a segurança de {}", caminho.display()))?;
    let mut texto = windows::core::PWSTR::null();
    // SAFETY: `descritor` veio do Windows e é válido; `texto` é liberado com `LocalFree`.
    let convertido = unsafe {
        ConvertSecurityDescriptorToStringSecurityDescriptorW(
            descritor,
            SDDL_REVISION_1,
            DACL_SECURITY_INFORMATION,
            std::ptr::from_mut(&mut texto),
            None,
        )
    };
    // SAFETY: as duas memórias vieram do Windows, que manda liberá-las com `LocalFree`.
    let resultado = convertido
        .context("convertendo o descritor em SDDL")
        .and_then(|()| unsafe { texto.to_string() }.context("SDDL fora de UTF-16"));
    unsafe {
        let _ = LocalFree(Some(HLOCAL(texto.0.cast())));
        let _ = LocalFree(Some(HLOCAL(descritor.0)));
    }
    resultado
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn a_pasta_protegida_para_de_herdar_e_fecha_o_que_ja_existia() {
        let pasta = std::env::temp_dir().join(format!("ir-acesso-dacl-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&pasta);
        std::fs::create_dir_all(&pasta).unwrap();
        // Como `%ProgramData%`: Usuários lê, e o que é criado dentro herda isso.
        proteger_pasta(&pasta, "D:P(A;OICI;FA;;;OW)(A;OICI;FR;;;BU)").unwrap();
        let chave = pasta.join("identity.key");
        std::fs::write(&chave, b"segredo").unwrap();
        assert!(dacl_em_sddl(&chave).unwrap().contains(";BU)"));

        // `OW` mantém o dono (quem roda o teste) capaz de apagar a pasta no fim.
        proteger_pasta(
            &pasta,
            "D:P(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)(A;OICI;FA;;;OW)",
        )
        .unwrap();

        let da_pasta = dacl_em_sddl(&pasta).unwrap();
        assert!(da_pasta.starts_with("D:P"), "{da_pasta}");
        let da_chave = dacl_em_sddl(&chave).unwrap();
        assert!(
            !da_chave.contains(";BU)"),
            "a chave antiga também fecha: {da_chave}"
        );
        assert!(!da_chave.contains(";AU)"), "{da_chave}");
        std::fs::remove_dir_all(&pasta).unwrap();
    }
}
