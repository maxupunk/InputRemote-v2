//! A pasta vista de quem a compartilhou: o índice que manda.
//!
//! A origem é o sequenciador. Cada mudança aceita — vista na varredura daqui ou vinda da réplica —
//! ganha o próximo número da pasta, e esse número é a versão da entrada. É por ele que a réplica
//! pede o que perdeu ([`Origem::mudancas_desde`]) e é contra ele que cada mudança da réplica é
//! conferida ([`Origem::decidir_envio`]).

use std::collections::BTreeMap;

use ir_proto::message::{Entry, EntryId, EntryKind, FolderId};
use serde::{Deserialize, Serialize};

use crate::ignorar::ignorar_caminho;
use crate::retrato::{Retrato, Visto, ancestrais};

mod decisao;

pub use decisao::{Contexto, Desfecho, EnvioRecebido};

/// O índice da origem.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Origem {
    pasta: FolderId,
    seq: u64,
    proximo_id: u64,
    /// Toda entrada que já existiu, inclusive as apagadas — a réplica que ficou fora precisa saber
    /// que uma coisa sumiu, e só a lápide diz isso.
    entradas: BTreeMap<String, Entry>,
}

impl Origem {
    /// Uma pasta recém-compartilhada, ainda sem nada no índice.
    #[must_use]
    pub const fn nova(pasta: FolderId) -> Self {
        Self {
            pasta,
            seq: 0,
            proximo_id: 1,
            entradas: BTreeMap::new(),
        }
    }

    /// A pasta.
    #[must_use]
    pub const fn pasta(&self) -> FolderId {
        self.pasta
    }

    /// O número atual da pasta.
    #[must_use]
    pub const fn seq(&self) -> u64 {
        self.seq
    }

    /// A entrada viva neste caminho.
    #[must_use]
    pub fn entrada(&self, caminho: &str) -> Option<&Entry> {
        self.entradas.get(caminho).filter(|e| !e.deleted)
    }

    /// A entrada viva com este identificador.
    #[must_use]
    pub fn por_id(&self, id: EntryId) -> Option<&Entry> {
        self.entradas.values().find(|e| e.id == id && !e.deleted)
    }

    /// Quantas entradas vivas a pasta tem, e a soma dos tamanhos — o que vai na oferta.
    #[must_use]
    pub fn tamanho(&self) -> (u32, u64) {
        let vivas = self.entradas.values().filter(|e| !e.deleted);
        vivas.fold((0, 0), |(n, bytes), e| {
            (n.saturating_add(1), bytes.saturating_add(e.size))
        })
    }

    /// Um arquivo vivo com este conteúdo, para gravar sem receber os bytes de novo.
    #[must_use]
    pub fn com_resumo(&self, resumo: &[u8; 32]) -> Option<&str> {
        self.entradas
            .iter()
            .find(|(_, e)| {
                !e.deleted && e.kind == EntryKind::File && e.hash.as_ref() == Some(resumo)
            })
            .map(|(caminho, _)| caminho.as_str())
    }

    /// Se a varredura precisa ler este arquivo para calcular o resumo.
    ///
    /// Só o que mudou por fora, ou o que nunca teve resumo: ler a pasta inteira a cada varredura
    /// custaria o disco.
    #[must_use]
    pub fn precisa_de_resumo(&self, caminho: &str, visto: &Visto) -> bool {
        if visto.eh_pasta() {
            return false;
        }
        match self.entrada(caminho) {
            Some(e) => e.hash.is_none() || !igual(e, visto),
            None => true,
        }
    }

    /// Aplica o que a varredura viu. Devolve as entradas que mudaram, já numeradas.
    ///
    /// Mudou o que aparece diferente por fora; sumiu o que está no índice e não no disco. O que
    /// [`crate::ignorar`] tira não entra nem quando está no retrato.
    pub fn aplicar_retrato(&mut self, retrato: &Retrato) -> Vec<Entry> {
        let mut mudou = Vec::new();
        for (caminho, visto) in retrato {
            if ignorar_caminho(caminho) {
                continue;
            }
            let igual_ao_indice = self
                .entrada(caminho)
                .is_some_and(|e| igual(e, visto) && (e.hash.is_some() || visto.resumo.is_none()));
            if !igual_ao_indice {
                mudou.push(self.gravar(caminho, visto));
            }
        }
        let sumiram: Vec<String> = self
            .entradas
            .iter()
            .filter(|(caminho, e)| !e.deleted && !retrato.contains_key(*caminho))
            .map(|(caminho, _)| caminho.clone())
            .collect();
        for caminho in sumiram {
            mudou.extend(self.apagar(&caminho));
        }
        mudou
    }

    /// Acerta o tamanho e o horário de uma entrada com o que o disco mostra depois de gravá-la, sem
    /// mudar a versão.
    ///
    /// Quem publica um arquivo da réplica não controla o horário exato que o sistema dá a ele; sem
    /// isto, a varredura seguinte veria "mudou" e o devolveria à réplica como mudança nova.
    pub fn acertar_com_o_disco(&mut self, caminho: &str, visto: &Visto) {
        if let Some(entrada) = self.entradas.get_mut(caminho).filter(|e| !e.deleted) {
            entrada.size = visto.tamanho;
            entrada.modified_ns = visto.modificado_ns;
            if visto.resumo.is_some() {
                entrada.hash = visto.resumo;
            }
        }
    }

    /// Tudo o que mudou depois de `desde`, em ordem de versão, lápides inclusive.
    #[must_use]
    pub fn mudancas_desde(&self, desde: u64) -> Vec<Entry> {
        let mut mudancas: Vec<Entry> = self
            .entradas
            .values()
            .filter(|e| e.version > desde)
            .cloned()
            .collect();
        mudancas.sort_by_key(|e| e.version);
        mudancas
    }

    /// O próximo número da pasta.
    fn proximo(&mut self) -> u64 {
        self.seq = self.seq.saturating_add(1);
        self.seq
    }

    /// Grava uma entrada nova ou mudada, numa versão nova. O identificador de uma entrada que já
    /// existiu fica o mesmo.
    fn gravar(&mut self, caminho: &str, visto: &Visto) -> Entry {
        let versao = self.proximo();
        let id = if let Some(antiga) = self.entradas.get(caminho) {
            antiga.id
        } else {
            let id = EntryId(self.proximo_id);
            self.proximo_id = self.proximo_id.saturating_add(1);
            id
        };
        let entrada = Entry {
            id,
            path: caminho.to_owned(),
            kind: visto.tipo,
            size: if visto.eh_pasta() { 0 } else { visto.tamanho },
            modified_ns: visto.modificado_ns,
            hash: visto.resumo,
            version: versao,
            deleted: false,
        };
        self.entradas.insert(caminho.to_owned(), entrada.clone());
        entrada
    }

    /// Põe lápide numa entrada viva — e, se for subpasta, em tudo o que está abaixo dela.
    fn apagar(&mut self, caminho: &str) -> Vec<Entry> {
        let alvos: Vec<String> = self
            .entradas
            .iter()
            .filter(|(c, e)| {
                !e.deleted && (c.as_str() == caminho || crate::retrato::dentro_de(c, caminho))
            })
            .map(|(c, _)| c.clone())
            .collect();
        let mut apagadas = Vec::new();
        for alvo in alvos {
            let versao = self.proximo();
            if let Some(entrada) = self.entradas.get_mut(&alvo) {
                entrada.deleted = true;
                entrada.version = versao;
                apagadas.push(entrada.clone());
            }
        }
        apagadas
    }

    /// As subpastas acima de `caminho` que ainda não existem no índice, criadas como entradas.
    fn garantir_pastas(&mut self, caminho: &str, agora_ns: i64) -> Vec<Entry> {
        let faltam: Vec<String> = ancestrais(caminho)
            .filter(|pasta| self.entrada(pasta).is_none())
            .map(str::to_owned)
            .collect();
        faltam
            .iter()
            .map(|pasta| self.gravar(pasta, &Visto::pasta(agora_ns)))
            .collect()
    }
}

/// Se a entrada do índice e o que o disco mostra são iguais por fora.
fn igual(entrada: &Entry, visto: &Visto) -> bool {
    let do_indice = Visto {
        tipo: entrada.kind,
        tamanho: entrada.size,
        modificado_ns: entrada.modified_ns,
        resumo: entrada.hash,
    };
    do_indice.igual_por_fora(visto)
}

#[cfg(test)]
mod testes;
