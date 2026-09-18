//! Quem pediu o envio só manda o que poderia ler sozinho.
//!
//! # O defeito que este módulo fecha
//!
//! O serviço roda como root no Linux e como SYSTEM no Windows. Quando um usuário pede "mande este
//! arquivo", é o **serviço** que abre o arquivo — com a autoridade dele, não a de quem pediu. Sem
//! esta verificação, qualquer membro do grupo `inputremote` pedia `/etc/shadow` e o recebia do outro
//! lado. É o *confused deputy* clássico, e mora exatamente no vetor que
//! [04, §2](../../../docs/04-seguranca.md) chama de "o mais provável e mais subestimado": o usuário
//! local sem privilégio usando o IPC.
//!
//! Foi introduzido junto com `Pedido::EnviarArquivos`, no commit `28492e7`, e corrigido antes de
//! qualquer pacote sair com ele.
//!
//! # A regra, e por que ela é conservadora
//!
//! Uma entrada passa se **pertence a quem pediu**, ou se **qualquer um** poderia ler — o bit de
//! "outros" do modo. Grupo não conta: o grupo do arquivo pode ser um de que o usuário faz parte,
//! mas descobrir isso de forma confiável exigiria consultar o banco de grupos na hora, e errar para
//! o lado de recusar é o lado certo. O custo é recusar um arquivo legível só por grupo, que o
//! usuário pode copiar para a própria pasta antes.
//!
//! Ler um arquivo exige também **atravessar** todas as pastas acima dele. Um `0644` dentro de um
//! `/root` fechado não é legível por ninguém além de root — e o bit do arquivo sozinho diria que é.
//!
//! # Onde a verificação é feita importa mais que a regra
//!
//! Conferir o caminho antes e abrir depois deixa uma janela: o usuário troca uma pasta por um
//! vínculo simbólico entre as duas coisas, e o serviço abre outro arquivo. Por isso o envio confere
//! **o arquivo já aberto** — o `fstat` do descritor, e o caminho real que o núcleo diz que aquele
//! descritor tem. O que se confere é o que se lê.
//!
//! # No Windows, quem decide é o próprio Windows
//!
//! Lá não há dono e modo: há ACL, com herança, grupos e negações. Reimplementar isso seria errar.
//! O serviço captura a identidade de quem pediu (o token do cliente do *pipe*) e pergunta ao sistema,
//! para cada objeto **já aberto**, se aquela identidade poderia ler — é o [`Leitor::PeloSistema`].
//! Pastas acima não são conferidas: no Windows, atravessar é um privilégio que todo usuário tem
//! (*bypass traverse checking*), e o que decide é a ACL do próprio arquivo.

use std::path::Path;
use std::sync::Arc;

use crate::error::{FileError, Result};

/// Pergunta ao sistema se quem pediu poderia ler um objeto já aberto.
///
/// Mora do lado de quem conhece a identidade — o serviço, com o token do cliente do *pipe*. Este
/// crate só a consulta, e por isso continua sem nenhum `unsafe`.
pub trait Autorizacao: Send + Sync + std::fmt::Debug {
    /// Se quem pediu poderia ler este arquivo — ou, se `pasta`, listar esta pasta.
    ///
    /// # Errors
    ///
    /// Erro do sistema ao consultar; quem chama trata como recusa.
    fn pode_ler(&self, aberto: &std::fs::File, pasta: bool) -> std::io::Result<bool>;
}

/// Com a autoridade de quem os arquivos são lidos.
#[derive(Debug, Clone)]
pub enum Leitor {
    /// Quem pediu tem a mesma autoridade que o serviço — o próprio dono do serviço, ou root.
    /// Não há fronteira a proteger.
    Proprio,
    /// Um usuário com menos autoridade que o serviço: só entra o que ele leria sozinho.
    Usuario {
        /// O `uid` de quem pediu.
        uid: u32,
    },
    /// Não se sabe quem pediu, e o serviço é privilegiado. Nada é lido.
    ///
    /// É o caso do serviço do Windows como SYSTEM até a verificação por personificação existir: sem
    /// saber quem está do outro lado do *pipe*, recusar é a única resposta segura.
    Desconhecido,
    /// O sistema confere cada objeto aberto com a identidade de quem pediu. É o Windows.
    PeloSistema(Arc<dyn Autorizacao>),
}

impl PartialEq for Leitor {
    fn eq(&self, outro: &Self) -> bool {
        match (self, outro) {
            (Self::Proprio, Self::Proprio) | (Self::Desconhecido, Self::Desconhecido) => true,
            (Self::Usuario { uid: a }, Self::Usuario { uid: b }) => a == b,
            (Self::PeloSistema(a), Self::PeloSistema(b)) => Arc::ptr_eq(a, b),
            _ => false,
        }
    }
}

impl Eq for Leitor {}

impl Leitor {
    /// Quem é o leitor, a partir de quem conectou ao serviço.
    ///
    /// - `chamador`: o `uid` de quem conectou, quando o sistema diz — o `SO_PEERCRED` no Linux. No
    ///   Windows o *pipe* não diz, e é `None`.
    /// - `servico`: o `uid` com que o serviço roda.
    /// - `privilegiado`: se o serviço tem mais autoridade que um usuário comum — root, SYSTEM.
    ///
    /// Sem saber quem conectou, um serviço privilegiado não lê nada por ninguém; um serviço rodando
    /// como o próprio usuário, sim, porque ali não há fronteira.
    #[must_use]
    pub const fn do_chamador(chamador: Option<u32>, servico: u32, privilegiado: bool) -> Self {
        match chamador {
            Some(uid) if uid == 0 || uid == servico => Self::Proprio,
            Some(uid) => Self::Usuario { uid },
            None if privilegiado => Self::Desconhecido,
            None => Self::Proprio,
        }
    }
}

/// O que se sabe de uma entrada do sistema de arquivos, para decidir.
///
/// Separado de `std::fs::Metadata` para a regra ser testável em qualquer sistema, inclusive onde
/// não há `uid` nem modo.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Dono {
    /// O dono da entrada.
    pub uid: u32,
    /// Os bits de permissão.
    pub modo: u32,
    /// Se é pasta.
    pub pasta: bool,
}

/// O que se quer fazer com a entrada.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Uso {
    /// Ler o conteúdo — ou, numa pasta, listar e entrar.
    Ler,
    /// Só passar por ela a caminho de outra coisa.
    Atravessar,
}

/// Se `leitor` pode fazer `uso` desta entrada.
#[must_use]
pub const fn permite(leitor: &Leitor, dono: Dono, uso: Uso) -> bool {
    match leitor {
        Leitor::Proprio => true,
        // Quem pergunta ao sistema não decide por dono e modo.
        Leitor::Desconhecido | Leitor::PeloSistema(_) => false,
        Leitor::Usuario { uid } => {
            if dono.uid == *uid {
                return true;
            }
            // Bits de "outros": leitura 0o004, execução (atravessar) 0o001.
            let precisa = match uso {
                Uso::Ler if dono.pasta => 0o005,
                Uso::Ler => 0o004,
                Uso::Atravessar => 0o001,
            };
            dono.modo & precisa == precisa
        }
    }
}

/// Recusa cedo, sem tocar o disco, quando não há como ler nada.
///
/// # Errors
///
/// [`FileError::SemPermissao`] para [`Leitor::Desconhecido`].
pub fn conferir_leitor(leitor: &Leitor, caminho: &Path) -> Result<()> {
    if *leitor == Leitor::Desconhecido {
        return Err(FileError::SemPermissao(caminho.to_path_buf()));
    }
    Ok(())
}

/// Confere só a entrada, sem as pastas acima.
///
/// Para o que está **dentro** de uma raiz já conferida: as pastas do caminho foram conferidas ao
/// entrar nelas, e refazer a subida a cada um de dez mil arquivos seria custo sem ganho.
///
/// # Errors
///
/// [`FileError::SemPermissao`] se o leitor não poderia ler a entrada.
pub fn conferir_entrada(
    leitor: &Leitor,
    caminho: &Path,
    metadados: &std::fs::Metadata,
) -> Result<()> {
    conferir_leitor(leitor, caminho)?;
    let pode = match leitor {
        Leitor::Proprio => true,
        Leitor::PeloSistema(sistema) => {
            // Aberta só para perguntar: o que se pergunta é sobre o objeto, e não sobre o nome.
            let aberta = abrir_para_perguntar(caminho, metadados.is_dir())
                .map_err(|erro| FileError::io(caminho, erro))?;
            pelo_sistema(sistema.as_ref(), &aberta, metadados.is_dir())
        }
        _ => permite(leitor, dono_de(metadados)?, Uso::Ler),
    };
    if pode {
        Ok(())
    } else {
        Err(FileError::SemPermissao(caminho.to_path_buf()))
    }
}

/// Confere uma entrada pelos metadados, e todas as pastas acima dela.
///
/// # Errors
///
/// [`FileError::SemPermissao`] se o leitor não poderia ler a entrada ou atravessar alguma pasta
/// acima dela.
pub fn conferir(leitor: &Leitor, caminho: &Path, metadados: &std::fs::Metadata) -> Result<()> {
    conferir_entrada(leitor, caminho, metadados)?;
    match leitor {
        Leitor::Usuario { .. } => conferir_acima(leitor, caminho),
        // Com a autoridade do serviço não há o que conferir; pelo sistema, atravessar não conta.
        _ => Ok(()),
    }
}

/// Confere que o leitor poderia atravessar todas as pastas acima do caminho.
fn conferir_acima(leitor: &Leitor, caminho: &Path) -> Result<()> {
    for pasta in caminho.ancestors().skip(1) {
        if pasta.as_os_str().is_empty() {
            continue;
        }
        let metadados = std::fs::metadata(pasta).map_err(|erro| FileError::io(pasta, erro))?;
        if !permite(leitor, dono_de(&metadados)?, Uso::Atravessar) {
            return Err(FileError::SemPermissao(caminho.to_path_buf()));
        }
    }
    Ok(())
}

/// Confere um arquivo **já aberto**: o que se confere é o que será lido.
///
/// No Linux o caminho real vem do próprio descritor (`/proc/self/fd`), e não do que foi pedido.
/// Assim uma pasta trocada por vínculo simbólico entre o manifesto e a abertura não leva a leitura
/// para outro lugar sem ser notada. No Windows a pergunta é feita ao sistema sobre o próprio objeto
/// aberto, o que dá a mesma garantia.
///
/// # Errors
///
/// [`FileError::SemPermissao`] se o arquivo aberto não seria legível por quem pediu.
pub fn conferir_aberto(leitor: &Leitor, pedido: &Path, arquivo: &std::fs::File) -> Result<()> {
    conferir_leitor(leitor, pedido)?;
    let pode = match leitor {
        Leitor::Proprio => return Ok(()),
        Leitor::PeloSistema(sistema) => pelo_sistema(sistema.as_ref(), arquivo, false),
        _ => {
            let metadados = arquivo
                .metadata()
                .map_err(|erro| FileError::io(pedido, erro))?;
            if !permite(leitor, dono_de(&metadados)?, Uso::Ler) {
                return Err(FileError::SemPermissao(pedido.to_path_buf()));
            }
            return conferir_acima(leitor, &caminho_real(arquivo, pedido)?);
        }
    };
    if pode {
        Ok(())
    } else {
        Err(FileError::SemPermissao(pedido.to_path_buf()))
    }
}

/// Abre uma entrada só para perguntar sobre ela.
///
/// No Windows uma pasta não abre como arquivo sem `FILE_FLAG_BACKUP_SEMANTICS`: sem o sinal, toda
/// pasta copiada seria recusada. O sinal não decide nada aqui — quem decide é a pergunta, feita
/// com a identidade de quem pediu.
fn abrir_para_perguntar(caminho: &Path, pasta: bool) -> std::io::Result<std::fs::File> {
    let mut opcoes = std::fs::OpenOptions::new();
    opcoes.read(true);
    #[cfg(windows)]
    if pasta {
        use std::os::windows::fs::OpenOptionsExt;
        /// `FILE_FLAG_BACKUP_SEMANTICS`, de `winbase.h`.
        const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;
        opcoes.custom_flags(FILE_FLAG_BACKUP_SEMANTICS);
    }
    #[cfg(not(windows))]
    let _ = pasta;
    opcoes.open(caminho)
}

/// Pergunta ao sistema. Um erro na pergunta é um "não": na dúvida, nada sai desta máquina.
fn pelo_sistema(sistema: &dyn Autorizacao, aberto: &std::fs::File, pasta: bool) -> bool {
    sistema.pode_ler(aberto, pasta).unwrap_or(false)
}

/// O caminho que o núcleo diz que este descritor tem.
#[cfg(target_os = "linux")]
fn caminho_real(arquivo: &std::fs::File, pedido: &Path) -> Result<std::path::PathBuf> {
    use std::os::fd::AsRawFd;
    let vinculo = format!("/proc/self/fd/{}", arquivo.as_raw_fd());
    std::fs::read_link(&vinculo).map_err(|erro| FileError::io(pedido, erro))
}

/// Fora do Linux não há `/proc`. Só chega aqui com [`Leitor::Usuario`], que só é produzido no Linux.
#[cfg(not(target_os = "linux"))]
fn caminho_real(_arquivo: &std::fs::File, pedido: &Path) -> Result<std::path::PathBuf> {
    Err(FileError::SemPermissao(pedido.to_path_buf()))
}

// `Result` por simetria com o caso sem `uid`, onde a resposta é recusar.
#[cfg(unix)]
#[allow(clippy::unnecessary_wraps)]
fn dono_de(metadados: &std::fs::Metadata) -> Result<Dono> {
    use std::os::unix::fs::MetadataExt;
    Ok(Dono {
        uid: metadados.uid(),
        modo: metadados.mode(),
        pasta: metadados.is_dir(),
    })
}

/// Sem `uid` e modo, não há como decidir por outro usuário — e na dúvida, não.
#[cfg(not(unix))]
fn dono_de(_metadados: &std::fs::Metadata) -> Result<Dono> {
    Err(FileError::SemPermissao(std::path::PathBuf::new()))
}

#[cfg(test)]
mod testes;
