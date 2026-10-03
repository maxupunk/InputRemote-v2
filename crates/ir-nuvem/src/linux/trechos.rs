//! O começo de um arquivo que não veio, lido sem baixá-lo.
//!
//! O Nautilus abre cada arquivo de extensão ambígua para ler os primeiros bytes e decidir o tipo —
//! no Fedora 44, todo `.png` (`image/png` ou `image/apng`). Se abrir baixasse o arquivo inteiro,
//! olhar uma pasta de fotos baixaria todas. Então quem lê só o começo recebe só o trecho, pedido à
//! origem como no Windows ([`crate::Pedido::Buscar`] com `offset` e `tamanho`), e o arquivo continua
//! sem conteúdo no disco.
//!
//! As funções [`entregar`] e [`falhar`] têm a forma das do Windows: o ajudante responde do mesmo
//! jeito nos dois sistemas, pelo número da transferência.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Condvar, Mutex};
use std::time::Duration;

/// Até onde uma leitura é "o começo": o que a detecção de tipo e a leitura antecipada do núcleo
/// pedem. Uma leitura que passa daqui traz o arquivo inteiro.
pub(super) const COMECO: u64 = 256 * 1024;

/// Quanto quem lê espera o trecho antes de ouvir "rede inalcançável".
const PRAZO: Duration = Duration::from_secs(60);

/// Um trecho pedido, e o que já chegou dele.
struct Trecho {
    inicio: u64,
    tamanho: u64,
    dados: Vec<u8>,
    falhou: bool,
}

impl Trecho {
    fn esperando(&self) -> bool {
        !self.falhou && (self.dados.len() as u64) < self.tamanho
    }
}

static TRECHOS: Mutex<BTreeMap<i64, Trecho>> = Mutex::new(BTreeMap::new());
static CHEGOU: Condvar = Condvar::new();
static PROXIMO: AtomicI64 = AtomicI64::new(1);

/// Pede `tamanho` bytes a partir de `inicio` — `pedir` recebe o número da transferência — e espera.
/// `None` quando não vem: sem o outro computador, ou passado o prazo.
pub(super) fn ler(pedir: impl FnOnce(i64), inicio: u64, tamanho: u64) -> Option<Vec<u8>> {
    let id = PROXIMO.fetch_add(1, Ordering::Relaxed);
    let trecho = Trecho {
        inicio,
        tamanho,
        dados: Vec::new(),
        falhou: false,
    };
    TRECHOS.lock().ok()?.insert(id, trecho);
    pedir(id);
    let trechos = TRECHOS.lock().ok()?;
    let (mut trechos, _) = CHEGOU
        .wait_timeout_while(trechos, PRAZO, |t| {
            t.get(&id).is_some_and(Trecho::esperando)
        })
        .ok()?;
    let trecho = trechos.remove(&id)?;
    (!trecho.esperando() && !trecho.falhou).then_some(trecho.dados)
}

/// Bytes de um trecho pedido, na ordem.
///
/// # Errors
///
/// Quando ninguém espera mais por essa transferência, ou o pedaço não é o próximo.
pub fn entregar(
    _conexao: i64,
    transferencia: i64,
    offset: u64,
    dados: &[u8],
) -> std::io::Result<()> {
    let mut trechos = TRECHOS
        .lock()
        .map_err(|_| std::io::Error::other("trechos envenenados"))?;
    let trecho = trechos
        .get_mut(&transferencia)
        .ok_or(std::io::ErrorKind::NotFound)?;
    if offset != trecho.inicio + trecho.dados.len() as u64 {
        return Err(std::io::ErrorKind::InvalidData.into());
    }
    trecho.dados.extend_from_slice(dados);
    drop(trechos);
    CHEGOU.notify_all();
    Ok(())
}

/// O trecho não vem: quem lê ouve "rede inalcançável".
///
/// # Errors
///
/// Nunca, hoje; a forma é a do Windows.
pub fn falhar(_conexao: i64, transferencia: i64, _faixa: (u64, u64)) -> std::io::Result<()> {
    if let Ok(mut trechos) = TRECHOS.lock()
        && let Some(trecho) = trechos.get_mut(&transferencia)
    {
        trecho.falhou = true;
    }
    CHEGOU.notify_all();
    Ok(())
}
