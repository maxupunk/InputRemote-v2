//! Quem cuida para a pasta de recebidos não encher o disco.
//!
//! O que chega precisa existir em algum lugar para o usuário poder colar — e, depois de colado,
//! sobra. Ninguém volta ali para apagar: na bancada a pasta chegou a 8 GB de vídeos e instaladores
//! que já tinham sido colados havia muito tempo.
//!
//! São dois momentos. **Ao subir o serviço, a pasta é esvaziada**: o clipboard não sobrevive ao
//! desligamento, então nada do que ficou ali ainda vai ser colado — guardar seria só ocupar disco.
//!
//! **Durante a sessão** a regra é a de uma pasta de downloads, não a de um arquivo: **o recente
//! fica, o antigo sai**. Três limites, nesta ordem de prioridade:
//!
//! 1. as entregas mais novas **nunca** são apagadas — a que acabou de chegar é a que o usuário está
//!    prestes a colar, e apagá-la seria o pior defeito possível;
//! 2. o que passou da idade sai, tenha o tamanho que tiver: uma entrega de três semanas não vai
//!    mais ser colada;
//! 3. se ainda passar do teto de espaço, sai da mais velha para a mais nova até caber.
//!
//! Quem decide é [`escolher`], que é pura e não toca o disco. O que toca o disco só executa a
//! decisão — e é o que permite testar a política inteira sem criar 8 GB de arquivos.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime};

use tracing::{debug, info};

/// Os limites da pasta de recebidos.
#[derive(Debug, Clone, Copy)]
pub struct Limites {
    /// O teto de espaço. Passou disto, as mais velhas saem.
    pub espaco: u64,
    /// A idade além da qual uma entrega sai, mesmo que caiba.
    pub idade: Duration,
    /// Quantas entregas recentes ficam sempre, custe o que custar.
    pub recentes: usize,
}

impl Default for Limites {
    /// Dois gigabytes, duas semanas, e as três mais novas sempre.
    ///
    /// O teto é generoso porque o produto existe para mover arquivo grande: um vídeo de 1,5 GB
    /// atravessou na bancada, e um teto apertado o apagaria antes de o usuário colar. A idade é o
    /// que resolve o caso real — a pasta não cresce por um arquivo grande, cresce por meses de
    /// arquivos pequenos que ninguém apagou.
    fn default() -> Self {
        Self {
            espaco: 2 * 1024 * 1024 * 1024,
            idade: Duration::from_secs(14 * 24 * 60 * 60),
            recentes: 3,
        }
    }
}

/// Quem cuida da pasta, e lembra quanto ela ocupa.
///
/// O tamanho fica guardado num número atômico porque quem o mostra é o ator do serviço, que **não
/// espera por disco**: ele gira a cada 5 ms. Medir uma pasta é caminhar por ela, e isso acontece
/// aqui, fora do caminho da entrada — depois de cada entrega e quando o usuário manda limpar.
#[derive(Debug)]
pub struct Faxineiro {
    pasta: PathBuf,
    limites: Limites,
    espaco: AtomicU64,
}

impl Faxineiro {
    /// Um faxineiro para esta pasta, com os limites dados.
    #[must_use]
    pub fn novo(pasta: PathBuf, limites: Limites) -> Arc<Self> {
        Arc::new(Self {
            pasta,
            limites,
            espaco: AtomicU64::new(0),
        })
    }

    /// Quanto a pasta ocupava na última medida. Leitura barata, para a tela.
    #[must_use]
    pub fn espaco(&self) -> u64 {
        self.espaco.load(Ordering::Relaxed)
    }

    /// Onde os recebidos ficam — o que a janela precisa para abrir a pasta.
    #[must_use]
    pub fn pasta(&self) -> &Path {
        &self.pasta
    }

    /// Aplica os limites e mede de novo. É o que roda depois de cada entrega.
    pub async fn arrumar(&self) -> Faxinado {
        let feito = arrumar(&self.pasta, self.limites).await;
        self.medir().await;
        feito
    }

    /// Esvazia a pasta. Roda ao subir o serviço e quando o usuário aperta "Limpar agora".
    pub async fn esvaziar(&self) -> Faxinado {
        let feito = esvaziar(&self.pasta).await;
        self.medir().await;
        info!(
            entregas = feito.entregas,
            bytes = feito.bytes,
            "recebidos esvaziados"
        );
        feito
    }

    /// Mede a pasta e guarda o resultado — só as entregas, e não a cópia que ainda está chegando.
    pub async fn medir(&self) {
        let bytes = listar(&self.pasta)
            .await
            .iter()
            .fold(0u64, |total, entrega| total.saturating_add(entrega.bytes));
        self.espaco.store(bytes, Ordering::Relaxed);
    }
}

/// Uma entrega na pasta de recebidos.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entrega {
    /// Onde ela está.
    pub caminho: PathBuf,
    /// Quanto ocupa, com tudo o que tem dentro.
    pub bytes: u64,
    /// Há quanto tempo chegou.
    pub idade: Duration,
}

/// O que a faxina fez.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Faxinado {
    /// Quantas entregas saíram.
    pub entregas: usize,
    /// Quanto espaço voltou.
    pub bytes: u64,
}

/// Quais entregas apagar, pelos limites dados. A lista devolvida está da mais velha para a mais
/// nova, que é a ordem em que apagar faz sentido.
#[must_use]
pub fn escolher(entregas: &[Entrega], limites: Limites) -> Vec<&Entrega> {
    let mut por_idade: Vec<&Entrega> = entregas.iter().collect();
    // Da mais nova para a mais velha: as primeiras são as intocáveis.
    por_idade.sort_by_key(|entrega| entrega.idade);
    let protegidas = limites.recentes.min(por_idade.len());
    let candidatas = por_idade.get(protegidas..).unwrap_or_default();

    let mut apagar: Vec<&Entrega> = Vec::new();
    let mut sobrando: u64 = por_idade.iter().map(|entrega| entrega.bytes).sum();
    for entrega in candidatas.iter().rev() {
        let velha = entrega.idade >= limites.idade;
        let cheia = sobrando > limites.espaco;
        if !velha && !cheia {
            continue;
        }
        sobrando = sobrando.saturating_sub(entrega.bytes);
        apagar.push(entrega);
    }
    apagar
}

/// Lê a pasta de recebidos: o que há nela, quanto ocupa e desde quando.
///
/// Uma entrega que não se consegue medir entra com zero byte e idade zero — ela não some da lista
/// por causa de um erro de leitura, e também não vira candidata a ser apagada por engano.
///
/// A montagem de uma cópia que ainda está chegando não é entrega e fica de fora
/// ([`ir_files::staging::e_montagem`]): "Limpar agora" a apagava no meio, e ela entrava na conta do
/// espaço antes de existir. A de um serviço morto no meio é recolhida pela próxima cópia de mesmo
/// identificador, ou pelo `Drop` de quem a criou.
pub async fn listar(pasta: &Path) -> Vec<Entrega> {
    let Ok(mut leitura) = tokio::fs::read_dir(pasta).await else {
        return Vec::new();
    };
    let agora = SystemTime::now();
    let mut entregas = Vec::new();
    while let Ok(Some(item)) = leitura.next_entry().await {
        if item
            .file_name()
            .to_str()
            .is_some_and(ir_files::staging::e_montagem)
        {
            continue;
        }
        let caminho = item.path();
        let bytes = Box::pin(tamanho(&caminho)).await;
        let idade = item
            .metadata()
            .await
            .ok()
            .and_then(|dados| dados.modified().ok())
            .and_then(|quando| agora.duration_since(quando).ok())
            .unwrap_or_default();
        entregas.push(Entrega {
            caminho,
            bytes,
            idade,
        });
    }
    entregas
}

/// Quanto um caminho ocupa, com tudo o que tem dentro.
pub async fn tamanho(caminho: &Path) -> u64 {
    let Ok(dados) = tokio::fs::metadata(caminho).await else {
        return 0;
    };
    if !dados.is_dir() {
        return dados.len();
    }
    let Ok(mut leitura) = tokio::fs::read_dir(caminho).await else {
        return 0;
    };
    let mut total: u64 = 0;
    while let Ok(Some(item)) = leitura.next_entry().await {
        total = total.saturating_add(Box::pin(tamanho(&item.path())).await);
    }
    total
}

/// Aplica os limites à pasta de recebidos.
pub async fn arrumar(pasta: &Path, limites: Limites) -> Faxinado {
    let entregas = listar(pasta).await;
    let alvos: Vec<Entrega> = escolher(&entregas, limites).into_iter().cloned().collect();
    apagar(&alvos).await
}

/// Apaga **tudo** o que há na pasta de recebidos. É o botão do usuário, e ele mandou.
pub async fn esvaziar(pasta: &Path) -> Faxinado {
    let entregas = listar(pasta).await;
    apagar(&entregas).await
}

/// Apaga as entregas dadas e conta o que saiu.
async fn apagar(entregas: &[Entrega]) -> Faxinado {
    let mut feito = Faxinado::default();
    for entrega in entregas {
        match ir_files::staging::remover(&entrega.caminho).await {
            Ok(()) => {
                // O nome do arquivo fica em `debug`, que é o teto de docs/04 §7.
                debug!(caminho = ?entrega.caminho, "entrega antiga apagada");
                feito.entregas += 1;
                feito.bytes = feito.bytes.saturating_add(entrega.bytes);
            }
            Err(erro) => debug!(%erro, "não consegui apagar uma entrega antiga"),
        }
    }
    if feito.entregas > 0 {
        info!(
            entregas = feito.entregas,
            bytes = feito.bytes,
            "recebidos: entregas antigas apagadas"
        );
    }
    feito
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;

    fn entrega(nome: &str, megabytes: u64, dias: u64) -> Entrega {
        Entrega {
            caminho: PathBuf::from(nome),
            bytes: megabytes * 1024 * 1024,
            idade: Duration::from_secs(dias * 24 * 60 * 60),
        }
    }

    fn nomes(escolhidas: &[&Entrega]) -> Vec<String> {
        escolhidas
            .iter()
            .map(|entrega| entrega.caminho.display().to_string())
            .collect()
    }

    #[test]
    fn uma_pasta_pequena_e_recente_nao_perde_nada() {
        let entregas = [entrega("a", 10, 0), entrega("b", 20, 1)];
        assert!(escolher(&entregas, Limites::default()).is_empty());
    }

    #[test]
    fn o_que_passou_da_idade_sai() {
        let entregas = [
            entrega("hoje", 10, 0),
            entrega("ontem", 10, 1),
            entrega("anteontem", 10, 2),
            entrega("mes-passado", 10, 30),
        ];
        let escolhidas = escolher(&entregas, Limites::default());
        assert_eq!(nomes(&escolhidas), ["mes-passado"]);
    }

    /// A que acabou de chegar é a que o usuário vai colar: ela não sai nem que estoure o teto.
    #[test]
    fn as_mais_novas_ficam_mesmo_estourando_o_espaco() {
        let limites = Limites {
            espaco: 1024,
            ..Limites::default()
        };
        let entregas = [
            entrega("agora", 2000, 0),
            entrega("recente", 2000, 1),
            entrega("outra", 2000, 2),
        ];
        assert!(
            escolher(&entregas, limites).is_empty(),
            "as três são as três mais novas"
        );
    }

    #[test]
    fn estourando_o_espaco_sai_da_mais_velha_ate_caber() {
        let limites = Limites {
            espaco: 100 * 1024 * 1024,
            idade: Duration::from_secs(u64::MAX / 2),
            recentes: 1,
        };
        let entregas = [
            entrega("nova", 40, 0),
            entrega("media", 40, 5),
            entrega("velha", 40, 10),
            entrega("velhissima", 40, 20),
        ];
        let escolhidas = escolher(&entregas, limites);
        // 160 MB no total, teto de 100 MB: saem as duas mais velhas, e para.
        assert_eq!(nomes(&escolhidas), ["velhissima", "velha"]);
    }

    #[test]
    fn a_ordem_de_apagar_e_da_mais_velha_para_a_mais_nova() {
        let limites = Limites {
            espaco: 0,
            idade: Duration::from_secs(u64::MAX / 2),
            recentes: 0,
        };
        let entregas = [entrega("b", 1, 5), entrega("c", 1, 9), entrega("a", 1, 1)];
        assert_eq!(nomes(&escolher(&entregas, limites)), ["c", "b", "a"]);
    }

    #[tokio::test]
    async fn esvaziar_apaga_arquivo_e_arvore_e_conta_o_que_saiu() {
        let pasta = std::env::temp_dir().join(format!("ir-faxina-{}", std::process::id()));
        let _ = tokio::fs::remove_dir_all(&pasta).await;
        tokio::fs::create_dir_all(pasta.join("arvore/dentro"))
            .await
            .expect("cria a árvore");
        tokio::fs::write(pasta.join("solto.bin"), vec![7; 2048])
            .await
            .expect("escreve o arquivo");
        tokio::fs::write(pasta.join("arvore/dentro/a.bin"), vec![7; 1024])
            .await
            .expect("escreve dentro da árvore");

        let feito = esvaziar(&pasta).await;
        assert_eq!(feito.entregas, 2);
        assert_eq!(feito.bytes, 3072);
        assert_eq!(listar(&pasta).await.len(), 0);
        let _ = tokio::fs::remove_dir_all(&pasta).await;
    }

    #[tokio::test]
    async fn a_copia_que_ainda_esta_chegando_nao_e_limpa_nem_contada() {
        // O defeito: "Limpar agora" apagava a montagem `.parcial-*` no meio de uma cópia, e o
        // espaço mostrado contava o que ainda nem tinha chegado.
        let pasta = std::env::temp_dir().join(format!("ir-faxina-parcial-{}", std::process::id()));
        let _ = tokio::fs::remove_dir_all(&pasta).await;
        let montagem = pasta.join(".parcial-7");
        tokio::fs::create_dir_all(&montagem)
            .await
            .expect("cria a montagem");
        tokio::fs::write(montagem.join("meio.bin"), vec![7; 4096])
            .await
            .expect("escreve na montagem");
        tokio::fs::write(pasta.join("entregue.bin"), vec![7; 1024])
            .await
            .expect("escreve a entrega");

        let entregas = listar(&pasta).await;
        assert_eq!(entregas.len(), 1, "{entregas:?}");
        let faxineiro = Faxineiro::novo(pasta.clone(), Limites::default());
        faxineiro.medir().await;
        assert_eq!(faxineiro.espaco(), 1024);

        let feito = faxineiro.esvaziar().await;
        assert_eq!(feito.entregas, 1);
        assert!(
            tokio::fs::metadata(montagem.join("meio.bin")).await.is_ok(),
            "a montagem sobreviveu à limpeza"
        );
        let _ = tokio::fs::remove_dir_all(&pasta).await;
    }
}
