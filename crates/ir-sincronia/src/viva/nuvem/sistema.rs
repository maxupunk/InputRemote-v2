//! O que a pasta sob demanda pede a cada sistema: os marcadores, o conteúdo, a desmontagem.
//!
//! No Windows, a Cloud Files API; no Linux, o cache por baixo da montagem FUSE. Separado da
//! conversa com a origem ([`super`]), que é a mesma nos dois.

#[cfg(windows)]
use tracing::{debug, info};

/// Tira do Windows as raízes deste usuário que não são de nenhuma pasta que existe.
pub(crate) fn limpar_orfas(manter: &[String]) {
    #[cfg(windows)]
    {
        let prefixo = ir_nuvem::id_da_raiz(&usuario(), "");
        let limpas = ir_nuvem::limpar_orfas(&prefixo, manter);
        if limpas > 0 {
            info!(limpas, "raízes sob demanda sem pasta tiradas do Windows");
        }
    }
    let _ = manter;
}

impl crate::viva::Viva {
    /// Diz à montagem do Linux se o outro computador está ao alcance: sem ele, abrir um arquivo
    /// que não veio falha na hora, com o motivo certo.
    #[cfg_attr(not(target_os = "linux"), allow(clippy::unused_self))] // só o Linux monta
    pub(crate) fn avisar_alcance(&self, conversando: bool) {
        #[cfg(target_os = "linux")]
        if let Some(montagem) = &self.nuvem.montagem {
            montagem.alcance(conversando);
        }
        let _ = conversando;
    }
}

#[cfg(any(windows, target_os = "linux"))]
pub(super) fn recusar(busca: &ir_nuvem::Busca) {
    let _ = ir_nuvem::falhar(
        busca.conexao,
        busca.transferencia,
        (busca.offset, busca.tamanho),
    );
}

#[cfg(not(any(windows, target_os = "linux")))]
pub(super) const fn recusar(_busca: &ir_nuvem::Busca) {}

#[cfg(any(windows, target_os = "linux"))]
pub(super) fn ir_nuvem_entregar(
    conexao: i64,
    transferencia: i64,
    offset: u64,
    dados: &[u8],
) -> std::io::Result<()> {
    ir_nuvem::entregar(conexao, transferencia, offset, dados)
}

#[cfg(not(any(windows, target_os = "linux")))]
pub(super) fn ir_nuvem_entregar(_c: i64, _t: i64, _o: u64, _d: &[u8]) -> std::io::Result<()> {
    Err(std::io::ErrorKind::Unsupported.into())
}

#[cfg(windows)]
pub(super) fn por_marcador(
    _raiz: &std::path::Path,
    destino: &std::path::Path,
    (tamanho, horario): (u64, i64),
) -> std::io::Result<()> {
    let existia = std::fs::symlink_metadata(destino).ok();
    match existia {
        Some(dados) => {
            let fixado = ir_nuvem::situacao(&dados).fixado;
            ir_nuvem::atualizar_marcador(destino, tamanho, horario)?;
            if fixado {
                hidratar_ao_fundo(destino.to_path_buf());
            }
            Ok(())
        }
        None => ir_nuvem::criar_marcador(destino, tamanho, horario, false),
    }
}

/// No Linux, o marcador é um arquivo esparso com o tamanho e a data, montado ao lado e posto no
/// lugar de uma vez, com a marca de sem conteúdo.
#[cfg(target_os = "linux")]
pub(super) fn por_marcador(
    raiz: &std::path::Path,
    destino: &std::path::Path,
    (tamanho, horario): (u64, i64),
) -> std::io::Result<()> {
    let montado = crate::disco::arquivo_de_montagem(
        raiz,
        &format!("marcador-{}", crate::disco::nanos_agora()),
    )?;
    std::fs::File::create(&montado)?.set_len(tamanho)?;
    ir_nuvem::marcar_sem_conteudo(&montado, true)?;
    crate::disco::publicar(&montado, destino, horario)
}

#[cfg(not(any(windows, target_os = "linux")))]
pub(super) fn por_marcador(
    _raiz: &std::path::Path,
    _destino: &std::path::Path,
    _metadados: (u64, i64),
) -> std::io::Result<()> {
    Err(std::io::ErrorKind::Unsupported.into())
}

/// Quando a pasta sob demanda do Linux deixa de ser compartilhada: o que veio passa do cache para a
/// pasta visível, como arquivo comum; o que não veio fica para trás — está inteiro na origem.
#[cfg(target_os = "linux")]
pub(super) fn trazer_do_cache(cache: &std::path::Path, visivel: &std::path::Path) {
    let mut pilha = vec![std::path::PathBuf::new()];
    while let Some(relativo) = pilha.pop() {
        let Ok(entradas) = std::fs::read_dir(cache.join(&relativo)) else {
            continue;
        };
        for entrada in entradas.filter_map(Result::ok) {
            if relativo.as_os_str().is_empty()
                && entrada.file_name() == ir_pasta::ignorar::PASTA_DE_CONTROLE
            {
                continue;
            }
            let caminho = relativo.join(entrada.file_name());
            if entrada.file_type().is_ok_and(|t| t.is_dir()) {
                let _ = std::fs::create_dir_all(visivel.join(&caminho));
                pilha.push(caminho);
            } else if !ir_nuvem::sem_conteudo(&entrada.path()) {
                let _ = crate::disco::mover(&entrada.path(), &visivel.join(&caminho));
            }
        }
    }
    let _ = std::fs::remove_dir_all(cache);
}

#[cfg(windows)]
pub(super) fn marcar_em_dia(destino: &std::path::Path) -> std::io::Result<()> {
    ir_nuvem::marcar_em_dia(destino)
}

#[cfg(not(windows))]
#[allow(clippy::unnecessary_wraps)] // a mesma assinatura da versão do Windows
pub(super) const fn marcar_em_dia(_destino: &std::path::Path) -> std::io::Result<()> {
    Ok(())
}

/// Traz o conteúdo numa thread própria: quem entrega os bytes é o laço, que não pode esperar.
#[cfg(windows)]
pub(super) fn hidratar_ao_fundo(caminho: std::path::PathBuf) {
    std::thread::spawn(move || {
        if let Err(erro) = ir_nuvem::hidratar(&caminho) {
            debug!(%erro, "o arquivo fixado não pôde ser trazido agora; tenta na próxima varredura");
        }
    });
}

/// Percorre a raiz e atende o que a pessoa pediu no Explorer.
#[cfg(windows)]
pub(super) fn fixados(raiz: &std::path::Path, limpo: impl Fn(&str) -> bool) {
    for (caminho, dados) in arquivos(raiz) {
        let situacao = ir_nuvem::situacao(&dados);
        let absoluto = crate::disco::absoluto(raiz, &caminho);
        if situacao.fixado && situacao.sem_conteudo {
            hidratar_ao_fundo(absoluto);
        } else if situacao.liberar && !situacao.sem_conteudo && limpo(&caminho) {
            let _ = ir_nuvem::desidratar(&absoluto);
        }
    }
}

#[cfg(not(windows))]
pub(super) fn fixados(_raiz: &std::path::Path, _limpo: impl Fn(&str) -> bool) {}

/// Antes de a raiz sair do Windows: o que veio fica como arquivo comum, o que não veio sai.
#[cfg(windows)]
pub(super) fn desmontar(raiz: &std::path::Path) {
    for (caminho, dados) in arquivos(raiz) {
        let absoluto = crate::disco::absoluto(raiz, &caminho);
        if ir_nuvem::situacao(&dados).sem_conteudo {
            let _ = std::fs::remove_file(&absoluto);
        } else {
            let _ = ir_nuvem::reverter(&absoluto);
        }
    }
}

/// Os arquivos da raiz, com os metadados, fora a pasta de controle.
#[cfg(windows)]
pub(super) fn arquivos(raiz: &std::path::Path) -> Vec<(String, std::fs::Metadata)> {
    let mut saida = Vec::new();
    let mut pilha = vec![(raiz.to_path_buf(), String::new())];
    while let Some((dir, prefixo)) = pilha.pop() {
        let Ok(entradas) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entrada in entradas.filter_map(Result::ok) {
            let nome = entrada.file_name().to_string_lossy().into_owned();
            if nome == ir_pasta::ignorar::PASTA_DE_CONTROLE {
                continue;
            }
            let Ok(dados) = std::fs::symlink_metadata(entrada.path()) else {
                continue;
            };
            let caminho = format!("{prefixo}{nome}");
            if dados.is_dir() {
                pilha.push((entrada.path(), format!("{caminho}/")));
            } else if dados.is_file() {
                saida.push((caminho, dados));
            }
        }
    }
    saida
}

/// O nome do usuário, para o identificador da raiz.
pub(super) fn usuario() -> String {
    std::env::var("USERNAME").unwrap_or_else(|_| "usuario".to_owned())
}

/// O ícone da pasta no painel: o da janela, ao lado deste executável.
#[cfg(windows)]
pub(super) fn icone() -> String {
    std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|pasta| pasta.join("inputremote-ui.exe")))
        .filter(|ui| ui.exists())
        .map_or_else(
            || r"%SystemRoot%\system32\imageres.dll,-1043".to_owned(),
            |ui| format!("{},0", ui.display()),
        )
}
