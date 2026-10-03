//! A réplica sob demanda do Windows: os arquivos aparecem como marcadores e o conteúdo vem quando
//! um programa os lê ([ADR-0015](../../../../docs/adr/0015-pastas-compartilhadas.md), §3).
//!
//! O que muda em relação à réplica que baixa tudo é pouco, e mora aqui:
//!
//! - uma mudança da origem vira **marcador** em vez de download — instantâneo, e a árvore inteira
//!   aparece mesmo offline;
//! - quando o Windows pede bytes de um marcador ([`ir_nuvem::Pedido::Buscar`]), a réplica pede o
//!   trecho à origem e entrega os pedaços à medida que chegam;
//! - offline, o pedido é recusado na hora: o programa ouve "rede indisponível", em vez de esperar;
//! - "Sempre manter neste dispositivo" traz o conteúdo; "Liberar espaço" o tira do disco.

use std::collections::BTreeMap;
use std::sync::Arc;

use ir_pasta::Baixar;
use ir_proto::limits::MAX_RANGE_REQUEST;
use ir_proto::message::{FolderMessage, RangeId};
use tracing::{debug, info, warn};

mod sistema;

use super::{Ambiente, Viva};
use crate::Saida;
use crate::guardado::Indice;
pub(crate) use sistema::limpar_orfas;
#[allow(clippy::wildcard_imports)]
use sistema::*;

/// Quem recebe os pedidos do Windows: o laço do ajudante.
pub type Repassar = Arc<dyn Fn(ir_nuvem::Pedido) + Send + Sync>;

/// Os números de pedido de trecho para o Windows ficam na metade de cima, longe dos downloads.
const BASE_DOS_PEDIDOS: u32 = 0x8000_0000;

/// O que a réplica sob demanda guarda enquanto vive.
#[derive(Default)]
pub(crate) struct Nuvem {
    #[cfg(windows)]
    conexao: Option<ir_nuvem::Conexao>,
    #[cfg(target_os = "linux")]
    montagem: Option<ir_nuvem::Montagem>,
    /// No Linux: os arquivos que alguém abriu e espera chegar.
    aguardando: std::collections::BTreeSet<String>,
    buscas: BTreeMap<RangeId, Busca>,
    proximo: u32,
}

impl std::fmt::Debug for Nuvem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Nuvem")
            .field("buscas", &self.buscas.len())
            .finish_non_exhaustive()
    }
}

/// Um pedido do Windows sendo atendido.
#[derive(Debug)]
struct Busca {
    pedido: ir_nuvem::Busca,
    entrada: ir_proto::message::EntryId,
    versao: u64,
    /// Até onde já se pediu à origem.
    pedido_ate: u64,
    fim: u64,
}

impl Viva {
    /// Registra a pasta como raiz de sincronia e conecta — quando ela é sob demanda.
    pub(crate) fn ligar_nuvem(&mut self, ambiente: &Ambiente, repassar: &Repassar) {
        let Indice::Replica(replica) = &self.guardada.indice else {
            return;
        };
        if !replica.sob_demanda() {
            return;
        }
        #[cfg(windows)]
        {
            let raiz = self.guardada.raiz.clone();
            let id = self.id_da_raiz();
            let nome = format!("{} (InputRemote)", self.guardada.nome);
            match ir_nuvem::registrar(&raiz, &id, &nome, &icone()) {
                Ok(None) => info!("pasta sob demanda registrada no Explorer"),
                Ok(Some(motivo)) => warn!(motivo, "pasta sob demanda sem o painel do Explorer"),
                Err(erro) => return warn!(%erro, "a pasta sob demanda não pôde ser registrada"),
            }
            let repassar = Arc::clone(repassar);
            match ir_nuvem::conectar(&raiz, move |pedido| repassar(pedido)) {
                Ok(conexao) => self.nuvem.conexao = Some(conexao),
                Err(erro) => warn!(%erro, "a pasta sob demanda não pôde ser conectada"),
            }
        }
        #[cfg(target_os = "linux")]
        if let Some(ponto) = self.guardada.ponto.clone() {
            let repassar = Arc::clone(repassar);
            match ir_nuvem::montar(&ponto, &self.guardada.raiz, move |pedido| repassar(pedido)) {
                Ok(montagem) => {
                    info!("pasta sob demanda montada");
                    self.nuvem.montagem = Some(montagem);
                }
                Err(erro) => warn!(%erro, "a pasta sob demanda não pôde ser montada"),
            }
        }
        let _ = (ambiente, repassar);
    }

    /// Alguém abriu, no Linux, um arquivo que ainda não veio: ele passa na frente da fila.
    fn buscar_por_inteiro(&mut self, busca: &ir_nuvem::Busca, ambiente: &Ambiente) {
        let Indice::Replica(replica) = &self.guardada.indice else {
            return;
        };
        let alvo = replica
            .remota(&busca.caminho)
            .map(baixar_de)
            .filter(|_| ambiente.conversando);
        match alvo {
            Some(alvo) => {
                self.nuvem.aguardando.insert(alvo.caminho.clone());
                self.baixas.enfileirar_na_frente(alvo);
            }
            None => self.avisar_quem_espera(&busca.caminho, false),
        }
    }

    /// Um download terminou bem: quem abriu o arquivo segue.
    pub(crate) fn download_chegou(&mut self, caminho: &str) {
        self.avisar_quem_espera(caminho, true);
    }

    /// Um download não vem: quem abriu o arquivo ouve "rede inalcançável".
    pub(crate) fn download_falhou(&mut self, caminho: &str) {
        self.avisar_quem_espera(caminho, false);
    }

    fn avisar_quem_espera(&mut self, caminho: &str, chegou: bool) {
        if !self.nuvem.aguardando.remove(caminho) && chegou {
            return;
        }
        #[cfg(target_os = "linux")]
        if let Some(montagem) = &self.nuvem.montagem {
            montagem.pronto(caminho, chegou);
        }
        let _ = chegou;
    }

    /// Numa pasta sob demanda, copiar um arquivo daqui para outro caminho leria um arquivo que
    /// talvez nem esteja no disco: a cópia vira marcador do que a origem tem no destino.
    pub(crate) fn sem_copia_sob_demanda(&self, acoes: Vec<ir_pasta::Acao>) -> Vec<ir_pasta::Acao> {
        let Indice::Replica(replica) = &self.guardada.indice else {
            return acoes;
        };
        if !replica.sob_demanda() {
            return acoes;
        }
        acoes
            .into_iter()
            .map(|acao| match acao {
                ir_pasta::Acao::Copiar { para, de } => match replica.remota(&para) {
                    Some(entrada) => ir_pasta::Acao::Marcador(baixar_de(entrada)),
                    None => ir_pasta::Acao::Copiar { de, para },
                },
                outra => outra,
            })
            .collect()
    }

    /// O Windows quer bytes de um marcador.
    pub(crate) fn buscar(
        &mut self,
        busca: ir_nuvem::Busca,
        ambiente: &Ambiente,
        saida: &mut dyn Saida,
    ) {
        // No Linux, quem lê além do começo pede o arquivo inteiro, que vem para o cache; quem lê só o
        // começo pede o trecho, como no Windows.
        if self.guardada.ponto.is_some() && busca.tamanho == u64::MAX {
            return self.buscar_por_inteiro(&busca, ambiente);
        }
        let Indice::Replica(replica) = &self.guardada.indice else {
            return;
        };
        let entrada = replica
            .remota(&busca.caminho)
            .map(|e| (e.id, e.version, e.size));
        let Some((entrada, versao, tamanho)) = entrada.filter(|_| ambiente.conversando) else {
            info!(
                conhecido = replica.remota(&busca.caminho).is_some(),
                conversando = ambiente.conversando,
                "pedido de conteúdo recusado na hora"
            );
            recusar(&busca);
            return;
        };
        debug!(
            offset = busca.offset,
            tamanho = busca.tamanho,
            "o Windows pediu conteúdo"
        );
        self.nuvem.proximo = self.nuvem.proximo.wrapping_add(1);
        let pedido = RangeId(BASE_DOS_PEDIDOS | (self.nuvem.proximo & !BASE_DOS_PEDIDOS));
        let fim = busca.offset.saturating_add(busca.tamanho).min(tamanho);
        let mut aberta = Busca {
            pedido: busca,
            entrada,
            versao,
            pedido_ate: 0,
            fim,
        };
        aberta.pedido_ate = aberta.pedido.offset;
        aberta.pedir(self.pasta(), pedido, saida);
        self.nuvem.buscas.insert(pedido, aberta);
    }

    /// O Windows desistiu de um pedido.
    pub(crate) fn cancelar_busca(&mut self, transferencia: i64, saida: &mut dyn Saida) {
        let canceladas: Vec<RangeId> = self
            .nuvem
            .buscas
            .iter()
            .filter(|(_, b)| b.pedido.transferencia == transferencia)
            .map(|(pedido, _)| *pedido)
            .collect();
        for pedido in canceladas {
            self.nuvem.buscas.remove(&pedido);
            saida.enviar(FolderMessage::CancelRange {
                folder: self.pasta(),
                request: pedido,
            });
        }
    }

    /// Um pedaço que a origem mandou. `false` quando ele não é de um pedido do Windows.
    pub(crate) fn trecho_da_nuvem(
        &mut self,
        (pedido, offset, dados): (RangeId, u64, &[u8]),
        saida: &mut dyn Saida,
    ) -> bool {
        let pasta = self.pasta();
        let Some(busca) = self.nuvem.buscas.get_mut(&pedido) else {
            return pedido.0 & BASE_DOS_PEDIDOS != 0;
        };
        let p = &busca.pedido;
        if let Err(erro) = ir_nuvem_entregar(p.conexao, p.transferencia, offset, dados) {
            // Vencido ou cancelado do lado do Windows: não adianta continuar.
            warn!(%erro, offset, tamanho = dados.len(), "o Windows recusou um pedaço do conteúdo");
            self.nuvem.buscas.remove(&pedido);
            return true;
        }
        let chegou_ate = offset + dados.len() as u64;
        if chegou_ate >= busca.fim {
            self.nuvem.buscas.remove(&pedido);
        } else if chegou_ate >= busca.pedido_ate {
            busca.pedir(pasta, pedido, saida);
        }
        true
    }

    /// A origem não pôde servir. `false` quando o pedido não é do Windows.
    pub(crate) fn falha_da_nuvem(&mut self, pedido: RangeId) -> bool {
        match self.nuvem.buscas.remove(&pedido) {
            Some(busca) => {
                recusar(&busca.pedido);
                true
            }
            None => pedido.0 & BASE_DOS_PEDIDOS != 0,
        }
    }

    /// O canal caiu: todo pedido em curso é recusado — o programa ouve "rede indisponível".
    pub(crate) fn soltar_buscas(&mut self) {
        for (_, busca) in std::mem::take(&mut self.nuvem.buscas) {
            recusar(&busca.pedido);
        }
        for caminho in std::mem::take(&mut self.nuvem.aguardando) {
            #[cfg(target_os = "linux")]
            if let Some(montagem) = &self.nuvem.montagem {
                montagem.pronto(&caminho, false);
            }
            let _ = caminho;
        }
    }

    /// Põe no disco o marcador de uma entrada da origem, e o índice passa a conhecê-lo.
    pub(crate) fn marcador(&mut self, alvo: &Baixar, ambiente: &Ambiente) {
        let destino = crate::disco::absoluto(&self.guardada.raiz, &alvo.caminho);
        let horario = alvo.modificado_ns.saturating_sub(ambiente.diferenca_ns);
        if let Err(erro) = por_marcador(&self.guardada.raiz, &destino, (alvo.tamanho, horario)) {
            return warn!(%erro, "não consegui pôr um arquivo sob demanda no lugar");
        }
        let Some(mut visto) = crate::varredura::visto_por_fora(&destino) else {
            return;
        };
        visto.resumo = alvo.resumo;
        if let Indice::Replica(replica) = &mut self.guardada.indice {
            replica.baixado(&alvo.caminho, alvo.versao, visto);
        }
        self.alterou();
    }

    /// Depois de uma mudança daqui ir para a origem: o ícone vira ✓.
    pub(crate) fn em_dia_na_nuvem(&self, caminho: &str) {
        let sob_demanda = matches!(&self.guardada.indice, Indice::Replica(r) if r.sob_demanda());
        if sob_demanda {
            let destino = crate::disco::absoluto(&self.guardada.raiz, caminho);
            if let Err(erro) = marcar_em_dia(&destino) {
                debug!(%erro, "não consegui marcar o arquivo em dia");
            }
        }
    }

    /// "Sempre manter neste dispositivo" e "Liberar espaço", pedidos no Explorer.
    pub(crate) fn cuidar_dos_fixados(&self) {
        let Indice::Replica(replica) = &self.guardada.indice else {
            return;
        };
        if !replica.sob_demanda() {
            return;
        }
        fixados(&self.guardada.raiz, |caminho| !replica.sujo(caminho));
    }

    /// A pasta deixa de ser sob demanda: o que já veio vira arquivo comum, o que não veio sai — está
    /// inteiro na origem —, e a raiz sai do Windows.
    ///
    /// Vale também sem conexão — na desinstalação, ninguém conectou —: basta a pasta ser sob demanda.
    pub(crate) fn desligar_nuvem(&mut self) {
        self.soltar_buscas();
        #[cfg(windows)]
        {
            drop(self.nuvem.conexao.take());
            if let Some(id) = self.raiz_sob_demanda() {
                desmontar(&self.guardada.raiz);
                ir_nuvem::desregistrar(&self.guardada.raiz, &id);
            }
        }
        #[cfg(target_os = "linux")]
        {
            drop(self.nuvem.montagem.take());
            if let Some(ponto) = self.guardada.ponto.clone() {
                trazer_do_cache(&self.guardada.raiz, &ponto);
            }
        }
    }
}

impl Viva {
    /// O identificador da raiz de sincronia desta pasta, quando ela é sob demanda.
    pub(crate) fn raiz_sob_demanda(&self) -> Option<String> {
        matches!(&self.guardada.indice, Indice::Replica(r) if r.sob_demanda())
            .then(|| self.id_da_raiz())
    }

    fn id_da_raiz(&self) -> String {
        ir_nuvem::id_da_raiz(&usuario(), &crate::lugar::hex(&self.pasta().0))
    }
}

impl Busca {
    fn pedir(
        &mut self,
        pasta: ir_proto::message::FolderId,
        pedido: RangeId,
        saida: &mut dyn Saida,
    ) {
        let falta = self.fim.saturating_sub(self.pedido_ate);
        let len = u32::try_from(falta)
            .unwrap_or(u32::MAX)
            .min(MAX_RANGE_REQUEST);
        if len == 0 {
            return;
        }
        saida.enviar(FolderMessage::RequestRange {
            folder: pasta,
            request: pedido,
            entry: self.entrada,
            version: self.versao,
            offset: self.pedido_ate,
            len,
        });
        self.pedido_ate += u64::from(len);
    }
}

/// Os dados de download de uma entrada da origem.
fn baixar_de(entrada: &ir_proto::message::Entry) -> Baixar {
    Baixar {
        caminho: entrada.path.clone(),
        entrada: entrada.id,
        versao: entrada.version,
        tamanho: entrada.size,
        resumo: entrada.hash,
        modificado_ns: entrada.modified_ns,
    }
}
