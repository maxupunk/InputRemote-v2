//! A varredura: o disco de uma pasta compartilhada virado em [`Retrato`].
//!
//! Lê só o que se vê por fora — tipo, tamanho, horário — e calcula o resumo apenas do que o motor
//! pede ([`ir_pasta::Origem::precisa_de_resumo`]): ler a pasta inteira a cada mudança custaria o
//! disco. Fica de fora:
//!
//! - ligação simbólica e junção — seguir uma delas leva a sincronia para fora da pasta. Outros
//!   pontos de nova análise ficam: um arquivo sob demanda (de um provedor de nuvem, inclusive o
//!   nosso) é um, e é arquivo de verdade para o usuário;
//! - o que [`ir_pasta::ignorar`] diz que não viaja;
//! - nome que não é UTF-8, ou que o Windows não grava (`a:b`, `aux.txt`, ponto no fim): contado em
//!   [`Varrida::invalidos`] para a janela dizer, e nunca transformado em silêncio noutro nome.

use std::io::Read;
use std::path::Path;
use std::time::UNIX_EPOCH;

use ir_pasta::ignorar::ignorar_nome;
use ir_pasta::{Retrato, Visto};
use ir_proto::message::data::is_safe_component;

/// O resultado de uma varredura.
#[derive(Debug, Default)]
pub struct Varrida {
    /// O que se viu.
    pub retrato: Retrato,
    /// Caminhos que não podem viajar pelo nome.
    pub invalidos: Vec<String>,
}

/// Varre `raiz`. `precisa_de_resumo` diz, para cada arquivo, se o conteúdo deve ser lido agora.
///
/// # Errors
///
/// Só quando a própria raiz não pode ser lida: sem ela, o retrato vazio pareceria "apagaram tudo".
pub fn varrer(
    raiz: &Path,
    precisa_de_resumo: &dyn Fn(&str, &Visto) -> bool,
) -> std::io::Result<Varrida> {
    std::fs::read_dir(raiz)?;
    let mut varrida = Varrida::default();
    descer(raiz, "", precisa_de_resumo, &mut varrida);
    Ok(varrida)
}

fn descer(
    dir: &Path,
    prefixo: &str,
    precisa_de_resumo: &dyn Fn(&str, &Visto) -> bool,
    varrida: &mut Varrida,
) {
    let Ok(entradas) = std::fs::read_dir(dir) else {
        return;
    };
    for entrada in entradas.filter_map(Result::ok) {
        let nome = entrada.file_name();
        let Some(nome) = nome.to_str() else {
            varrida
                .invalidos
                .push(format!("{prefixo}{}", nome.to_string_lossy()));
            continue;
        };
        if ignorar_nome(nome) {
            continue;
        }
        let caminho = format!("{prefixo}{nome}");
        if !is_safe_component(nome) {
            varrida.invalidos.push(caminho);
            continue;
        }
        let Ok(dados) = std::fs::symlink_metadata(entrada.path()) else {
            continue;
        };
        // No Windows, `is_symlink` é verdadeiro para todo ponto de nova análise que é "substituto de
        // nome" — ligação simbólica, junção, ponto de montagem —, e só para eles.
        if dados.file_type().is_symlink() {
            continue;
        }
        let modificado = horario(&dados);
        if dados.is_dir() {
            varrida
                .retrato
                .insert(caminho.clone(), Visto::pasta(modificado));
            descer(
                &entrada.path(),
                &format!("{caminho}/"),
                precisa_de_resumo,
                varrida,
            );
        } else if dados.is_file() {
            let mut visto = Visto::arquivo(dados.len(), modificado, None);
            // Um arquivo sob demanda que ainda não veio não é lido: ler o baixaria.
            if precisa_de_resumo(&caminho, &visto)
                && !nao_baixado(&dados)
                && !sem_conteudo(&entrada.path())
            {
                // Travado agora por outro programa: entra sem resumo, e a próxima varredura tenta.
                visto.resumo = resumir(&entrada.path()).ok();
            }
            varrida.retrato.insert(caminho, visto);
        }
    }
}

/// O horário de modificação em nanossegundos desde 1970; zero se o sistema não disser.
#[must_use]
pub fn horario(dados: &std::fs::Metadata) -> i64 {
    dados
        .modified()
        .ok()
        .and_then(|quando| quando.duration_since(UNIX_EPOCH).ok())
        .and_then(|desde| i64::try_from(desde.as_nanos()).ok())
        .unwrap_or(0)
}

/// O BLAKE3 do arquivo inteiro.
///
/// # Errors
///
/// Erro de leitura.
pub fn resumir(caminho: &Path) -> std::io::Result<[u8; 32]> {
    let mut arquivo = std::fs::File::open(caminho)?;
    let mut resumo = blake3::Hasher::new();
    let mut bloco = vec![0u8; 256 * 1024];
    loop {
        let lidos = arquivo.read(&mut bloco)?;
        if lidos == 0 {
            break;
        }
        resumo.update(bloco.get(..lidos).unwrap_or_default());
    }
    Ok(*resumo.finalize().as_bytes())
}

/// O que o disco mostra num caminho, já com o resumo, quando é arquivo.
#[must_use]
pub fn visto_de(caminho: &Path) -> Option<Visto> {
    let dados = std::fs::symlink_metadata(caminho).ok()?;
    let modificado = horario(&dados);
    if dados.is_dir() {
        return Some(Visto::pasta(modificado));
    }
    let resumo = resumir(caminho).ok();
    Some(Visto::arquivo(dados.len(), modificado, resumo))
}

/// O que o disco mostra num caminho, sem ler o conteúdo.
#[must_use]
pub fn visto_por_fora(caminho: &Path) -> Option<Visto> {
    let dados = std::fs::symlink_metadata(caminho).ok()?;
    let modificado = horario(&dados);
    Some(if dados.is_dir() {
        Visto::pasta(modificado)
    } else {
        Visto::arquivo(dados.len(), modificado, None)
    })
}

/// Se é um arquivo sob demanda cujo conteúdo ainda não está no disco: abrir para ler o traria.
#[cfg(windows)]
#[must_use]
pub fn nao_baixado(dados: &std::fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    const FILE_ATTRIBUTE_RECALL_ON_OPEN: u32 = 0x0004_0000;
    const FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS: u32 = 0x0040_0000;
    dados.file_attributes() & (FILE_ATTRIBUTE_RECALL_ON_OPEN | FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS)
        != 0
}

/// No Linux quem diz é a marca no próprio arquivo ([`sem_conteudo`]).
#[cfg(not(windows))]
#[must_use]
pub const fn nao_baixado(_dados: &std::fs::Metadata) -> bool {
    false
}

/// Se o arquivo do cache da pasta sob demanda do Linux ainda não tem o conteúdo.
#[cfg(target_os = "linux")]
fn sem_conteudo(caminho: &Path) -> bool {
    ir_nuvem::sem_conteudo(caminho)
}

#[cfg(not(target_os = "linux"))]
const fn sem_conteudo(_caminho: &Path) -> bool {
    false
}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn a_varredura_ve_arquivos_e_subpastas_e_deixa_de_fora_o_que_nao_viaja() {
        let raiz = std::env::temp_dir().join(format!("ir-varredura-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&raiz);
        std::fs::create_dir_all(raiz.join("sub/fundo")).unwrap();
        std::fs::write(raiz.join("a.txt"), b"a").unwrap();
        std::fs::write(raiz.join("sub/fundo/b.bin"), b"bb").unwrap();
        std::fs::write(raiz.join("~$trava.docx"), b"x").unwrap();
        std::fs::create_dir_all(raiz.join(".inputremote/lixeira")).unwrap();
        let varrida = varrer(&raiz, &|_, _| true).unwrap();
        let caminhos: Vec<&str> = varrida.retrato.keys().map(String::as_str).collect();
        assert_eq!(
            caminhos,
            vec!["a.txt", "sub", "sub/fundo", "sub/fundo/b.bin"]
        );
        let b = varrida.retrato["sub/fundo/b.bin"];
        assert_eq!(b.tamanho, 2);
        assert_eq!(b.resumo, Some(*blake3::hash(b"bb").as_bytes()));
        let sem_resumo = varrer(&raiz, &|_, _| false).unwrap();
        assert!(sem_resumo.retrato["a.txt"].resumo.is_none());
        let _ = std::fs::remove_dir_all(&raiz);
    }

    #[test]
    fn a_raiz_que_sumiu_e_erro_e_nao_retrato_vazio() {
        let raiz = std::env::temp_dir().join("ir-varredura-que-nao-existe");
        assert!(varrer(&raiz, &|_, _| true).is_err());
    }
}
