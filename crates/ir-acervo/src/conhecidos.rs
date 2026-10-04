//! Conteúdos que já estão neste computador, fora da pasta compartilhada: o que a pessoa copiou —
//! e a cópia levou ao outro computador — e o que chegou pela cópia.
//!
//! Copiar um arquivo num computador e colá-lo na pasta compartilhada do outro faria o mesmo
//! conteúdo atravessar a rede duas vezes: uma pela cópia, outra pela sincronia, ao mesmo tempo e
//! disputando o mesmo enlace. Os dois computadores já têm os bytes — um tem o original, o outro o
//! que chegou —, então quando a pasta precisa de um conteúdo, o ajudante procura aqui antes de
//! pedi-lo à rede: um arquivo do mesmo tamanho cujo BLAKE3 é o mesmo (o da cópia e o da pasta são a
//! mesma conta sobre os mesmos bytes).
//!
//! Quem escreve a lista é o ajudante de clipboard; quem a lê é o das pastas. Os dois rodam como o
//! mesmo usuário, e a lista mora na pasta de estado dele: nada atravessa de um usuário para outro.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// O nome da lista, na pasta de estado das pastas do usuário.
const ARQUIVO: &str = "conhecidos";

/// Quantos caminhos a lista guarda: os mais recentes ficam.
const MAXIMO: usize = 500;

/// Quanto tempo um caminho vale: a cópia de ontem não é a de agora.
const VALIDADE: Duration = Duration::from_secs(24 * 3_600);

/// Quantos arquivos de uma pasta copiada entram, no máximo.
const POR_PASTA: usize = 2_000;

/// O resumo de cada arquivo já conferido, pelo caminho, tamanho e horário: não reler o que não mudou.
type Chave = (PathBuf, u64, i64);
static RESUMOS: Mutex<Option<HashMap<Chave, [u8; 32]>>> = Mutex::new(None);

/// Acrescenta à lista estes caminhos — arquivos, ou pastas, que são percorridas.
pub fn registrar(estado: &Path, caminhos: &[PathBuf]) {
    let agora = nanos(SystemTime::now());
    let mut novos = Vec::new();
    for caminho in caminhos {
        arquivos_de(caminho, &mut novos);
    }
    if novos.is_empty() {
        return;
    }
    let mut lista: Vec<(i64, PathBuf)> = ler(estado)
        .into_iter()
        .filter(|(_, caminho)| !novos.contains(caminho))
        .collect();
    lista.extend(novos.into_iter().map(|caminho| (agora, caminho)));
    let sobra = lista.len().saturating_sub(MAXIMO);
    lista.drain(..sobra);
    if let Err(erro) = gravar(estado, &lista) {
        tracing::debug!(%erro, "não consegui guardar a lista do que foi copiado");
    }
}

/// Um arquivo deste computador, fora da pasta, com exatamente este conteúdo.
#[must_use]
pub fn achar(estado: &Path, resumo: &[u8; 32], tamanho: u64) -> Option<PathBuf> {
    ler(estado).into_iter().rev().find_map(|(_, caminho)| {
        let dados = std::fs::metadata(&caminho).ok()?;
        if !dados.is_file() || dados.len() != tamanho {
            return None;
        }
        let chave = (
            caminho.clone(),
            dados.len(),
            dados.modified().map_or(0, nanos),
        );
        (resumo_de(chave)? == *resumo).then_some(caminho)
    })
}

/// Copia `de` para `para` conferindo o resumo no caminho: o original pode ter mudado desde que foi
/// conferido. `false` se não bateu, e então `para` não fica.
///
/// # Errors
///
/// Erro de disco.
pub fn copiar_conferindo(de: &Path, para: &Path, resumo: &[u8; 32]) -> std::io::Result<bool> {
    let mut entrada = std::fs::File::open(de)?;
    let mut saida = std::fs::File::create(para)?;
    let mut conta = blake3::Hasher::new();
    let mut bloco = vec![0u8; 256 * 1024];
    loop {
        let lidos = entrada.read(&mut bloco)?;
        if lidos == 0 {
            break;
        }
        let pedaco = bloco.get(..lidos).unwrap_or_default();
        conta.update(pedaco);
        saida.write_all(pedaco)?;
    }
    saida.sync_all()?;
    drop(saida);
    let bateu = conta.finalize().as_bytes() == resumo;
    if !bateu {
        let _ = std::fs::remove_file(para);
    }
    Ok(bateu)
}

/// Atende um pedido de conteúdo com um arquivo deste computador, em pedaços: `false` se não deu
/// (então o pedido segue para a origem).
#[must_use]
pub fn servir(busca: &ir_nuvem::Busca, fonte: &std::path::Path, tamanho: u64) -> bool {
    use std::io::{Read, Seek, SeekFrom};
    let fim = busca.offset.saturating_add(busca.tamanho).min(tamanho);
    let Ok(mut arquivo) = std::fs::File::open(fonte) else {
        return false;
    };
    if arquivo.seek(SeekFrom::Start(busca.offset)).is_err() {
        return false;
    }
    let mut offset = busca.offset;
    let mut bloco = vec![0u8; 1024 * 1024];
    while offset < fim {
        let quer = usize::try_from(fim - offset).map_or(bloco.len(), |f| f.min(bloco.len()));
        let Some(pedaco) = bloco.get_mut(..quer) else {
            return false;
        };
        if arquivo.read_exact(pedaco).is_err()
            || entregar(busca.conexao, busca.transferencia, offset, pedaco).is_err()
        {
            return false;
        }
        offset += quer as u64;
    }
    tracing::info!("conteúdo servido de um arquivo deste computador; não atravessou a rede");
    true
}

#[cfg(any(windows, target_os = "linux"))]
use ir_nuvem::entregar;

/// Onde não há pasta sob demanda, não há a quem entregar.
#[cfg(not(any(windows, target_os = "linux")))]
fn entregar(_c: i64, _t: i64, _o: u64, _d: &[u8]) -> std::io::Result<()> {
    Err(std::io::ErrorKind::Unsupported.into())
}

fn resumo_de(chave: Chave) -> Option<[u8; 32]> {
    if let Some(resumo) = RESUMOS
        .lock()
        .ok()
        .and_then(|guarda| guarda.as_ref().and_then(|m| m.get(&chave).copied()))
    {
        return Some(resumo);
    }
    let resumo = crate::varredura::resumir(&chave.0).ok()?;
    if let Ok(mut guarda) = RESUMOS.lock() {
        guarda
            .get_or_insert_with(HashMap::new)
            .insert(chave, resumo);
    }
    Some(resumo)
}

/// Os arquivos de um caminho: ele mesmo, ou os de dentro, se for pasta.
fn arquivos_de(caminho: &Path, saida: &mut Vec<PathBuf>) {
    let mut pilha = vec![caminho.to_path_buf()];
    let mut vistos = 0;
    while let Some(atual) = pilha.pop() {
        let Ok(dados) = std::fs::symlink_metadata(&atual) else {
            continue;
        };
        if dados.is_file() {
            if cabe_na_lista(&atual) && !saida.contains(&atual) {
                saida.push(atual);
            }
            vistos += 1;
            if vistos >= POR_PASTA {
                return;
            }
        } else if dados.is_dir()
            && let Ok(entradas) = std::fs::read_dir(&atual)
        {
            pilha.extend(entradas.filter_map(Result::ok).map(|e| e.path()));
        }
    }
}

/// Uma linha por caminho: um caminho com quebra de linha ou tabulação não entra.
fn cabe_na_lista(caminho: &Path) -> bool {
    caminho
        .to_str()
        .is_some_and(|texto| !texto.contains(['\n', '\t', '\r']))
}

/// A lista, sem o que passou da validade.
fn ler(estado: &Path) -> Vec<(i64, PathBuf)> {
    let Ok(texto) = std::fs::read_to_string(estado.join(ARQUIVO)) else {
        return Vec::new();
    };
    let limite = nanos(SystemTime::now())
        .saturating_sub(i64::try_from(VALIDADE.as_nanos()).unwrap_or(i64::MAX));
    texto
        .lines()
        .filter_map(|linha| {
            let (quando, caminho) = linha.split_once('\t')?;
            let quando: i64 = quando.parse().ok()?;
            (quando >= limite).then(|| (quando, PathBuf::from(caminho)))
        })
        .collect()
}

/// Grava ao lado e troca: quem lê no meio vê a lista velha ou a nova.
fn gravar(estado: &Path, lista: &[(i64, PathBuf)]) -> std::io::Result<()> {
    std::fs::create_dir_all(estado)?;
    let mut texto = String::new();
    for (quando, caminho) in lista {
        if let Some(caminho) = caminho.to_str() {
            texto.push_str(&quando.to_string());
            texto.push('\t');
            texto.push_str(caminho);
            texto.push('\n');
        }
    }
    let novo = estado.join(format!("{ARQUIVO}.novo"));
    std::fs::write(&novo, texto)?;
    std::fs::rename(novo, estado.join(ARQUIVO))
}

fn nanos(quando: SystemTime) -> i64 {
    quando
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|d| i64::try_from(d.as_nanos()).ok())
        .unwrap_or(0)
}

#[cfg(test)]
mod testes {
    use super::*;

    fn pasta(nome: &str) -> PathBuf {
        let base =
            std::env::temp_dir().join(format!("ir-conhecidos-{nome}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        base
    }

    #[test]
    fn o_que_foi_copiado_e_achado_pelo_conteudo_e_so_se_nao_mudou() {
        let base = pasta("achar");
        let estado = base.join("estado");
        let foto = base.join("img2.jpg");
        std::fs::write(&foto, b"os bytes da foto").unwrap();
        std::fs::create_dir_all(base.join("pasta/sub")).unwrap();
        std::fs::write(base.join("pasta/sub/dentro.txt"), b"de uma pasta copiada").unwrap();
        registrar(&estado, &[foto.clone(), base.join("pasta")]);

        let resumo = *blake3::hash(b"os bytes da foto").as_bytes();
        assert_eq!(achar(&estado, &resumo, 16), Some(foto.clone()));
        let dentro = *blake3::hash(b"de uma pasta copiada").as_bytes();
        assert!(
            achar(&estado, &dentro, 20).is_some(),
            "o que estava numa pasta copiada"
        );
        assert_eq!(achar(&estado, &resumo, 17), None, "outro tamanho");

        // Mudou depois de copiado: não serve mais.
        std::fs::write(&foto, b"outros bytes, mud").unwrap();
        assert_eq!(achar(&estado, &resumo, 16), None);

        // A cópia conferida pega a troca no meio.
        let destino = base.join("copia");
        assert!(!copiar_conferindo(&foto, &destino, &resumo).unwrap());
        assert!(!destino.exists());
        let _ = std::fs::remove_dir_all(&base);
    }
}
