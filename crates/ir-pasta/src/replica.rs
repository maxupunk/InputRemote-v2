//! A pasta vista de quem a recebeu: o que a origem tem, o que há no disco daqui, e as mudanças daqui
//! que esperam para ir.
//!
//! A réplica nunca decide conflito — quem decide é a origem, que tem a ordem. O que a réplica faz é
//! não estragar nada até a decisão chegar: um arquivo mudado aqui (com operação pendente) não é
//! sobrescrito pelo que vem de lá. A mudança daqui vai com a versão em que se baseou, e a origem diz
//! o que houve.
//!
//! Offline, as mudanças ficam na fila ([`Replica::pendentes`]), que é guardada em disco junto com o
//! índice, e saem quando o canal voltar.

use std::collections::BTreeMap;

use ir_proto::message::{Entry, EntryKind, FolderId, OpId, OpResult, Refusal};
use serde::{Deserialize, Serialize};

use crate::acao::{Acao, Baixar};
use crate::retrato::Visto;

mod mudancas;
mod varrida;

/// O que esta réplica sabe de um caminho no disco daqui.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Local {
    /// Como o disco o mostrou da última vez.
    pub visto: Visto,
    /// A versão da origem a que este conteúdo corresponde; zero para o que nasceu aqui e a origem
    /// ainda não confirmou.
    pub base: u64,
}

/// Uma mudança daqui que espera para ir à origem.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Pendente {
    /// Mandar o conteúdo deste arquivo.
    Enviar(String),
    /// Este caminho foi apagado aqui; ele estava nesta versão.
    Apagar(String, u64),
    /// Esta subpasta foi criada aqui.
    CriarPasta(String),
}

impl Pendente {
    /// O caminho em que a mudança mexe.
    #[must_use]
    pub fn caminho(&self) -> &str {
        match self {
            Self::Enviar(c) | Self::Apagar(c, _) | Self::CriarPasta(c) => c,
        }
    }
}

/// Uma operação pronta para sair, com o número dela.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Operacao {
    /// Mandar o arquivo, baseado nesta versão.
    Enviar {
        /// O caminho.
        caminho: String,
        /// A versão em que o conteúdo daqui se baseou.
        base: u64,
    },
    /// Apagar.
    Apagar {
        /// O caminho.
        caminho: String,
        /// A versão apagada.
        base: u64,
    },
    /// Criar a subpasta.
    CriarPasta {
        /// O caminho.
        caminho: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct NaFila {
    pendente: Pendente,
    /// O número, quando já saiu e espera resposta.
    em_voo: Option<OpId>,
}

/// O índice da réplica.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Replica {
    pasta: FolderId,
    sob_demanda: bool,
    visto_ate: u64,
    /// As entradas vivas da origem, como a última leva de mudanças as deixou.
    remotas: BTreeMap<String, Entry>,
    /// O que há no disco daqui.
    locais: BTreeMap<String, Local>,
    fila: Vec<NaFila>,
    proximo_op: u32,
    /// Quantas mudanças daqui a origem recusou e não vão mais, para a janela contar.
    recusadas: u32,
}

impl Replica {
    /// Uma pasta recém-aceita, vazia. `sob_demanda` é o modo do Windows: o conteúdo só vem quando o
    /// arquivo é aberto ([`Acao::Marcador`] em vez de [`Acao::Baixar`]).
    #[must_use]
    pub const fn nova(pasta: FolderId, sob_demanda: bool) -> Self {
        Self {
            pasta,
            sob_demanda,
            visto_ate: 0,
            remotas: BTreeMap::new(),
            locais: BTreeMap::new(),
            fila: Vec::new(),
            proximo_op: 1,
            recusadas: 0,
        }
    }

    /// A pasta.
    #[must_use]
    pub const fn pasta(&self) -> FolderId {
        self.pasta
    }

    /// O número da pasta até onde esta réplica aplicou tudo.
    #[must_use]
    pub const fn visto_ate(&self) -> u64 {
        self.visto_ate
    }

    /// Se o conteúdo vem só quando o arquivo é aberto.
    #[must_use]
    pub const fn sob_demanda(&self) -> bool {
        self.sob_demanda
    }

    /// Quantas mudanças daqui esperam para ir.
    #[must_use]
    pub fn pendentes(&self) -> usize {
        self.fila.len()
    }

    /// Quantas mudanças daqui a origem recusou.
    #[must_use]
    pub const fn recusadas(&self) -> u32 {
        self.recusadas
    }

    /// A entrada da origem neste caminho.
    #[must_use]
    pub fn remota(&self, caminho: &str) -> Option<&Entry> {
        self.remotas.get(caminho)
    }

    /// O que esta réplica sabe do disco neste caminho.
    #[must_use]
    pub fn local(&self, caminho: &str) -> Option<&Local> {
        self.locais.get(caminho)
    }

    /// Se há mudança daqui esperando neste caminho — e então nada de lá pode sobrescrevê-lo.
    #[must_use]
    pub fn sujo(&self, caminho: &str) -> bool {
        self.fila.iter().any(|f| f.pendente.caminho() == caminho)
    }

    /// A próxima mudança daqui a mandar, já numerada. `None` quando não há, ou quando a anterior
    /// ainda espera resposta — uma por vez, para a ordem da fila valer na origem.
    pub fn proxima(&mut self) -> Option<(OpId, Operacao)> {
        if self.fila.iter().any(|f| f.em_voo.is_some()) {
            return None;
        }
        let op = OpId(self.proximo_op);
        let primeira = self.fila.first_mut()?;
        primeira.em_voo = Some(op);
        self.proximo_op = self.proximo_op.wrapping_add(1).max(1);
        let operacao = match &primeira.pendente {
            Pendente::Enviar(caminho) => Operacao::Enviar {
                caminho: caminho.clone(),
                base: self.locais.get(caminho).map_or(0, |l| l.base),
            },
            Pendente::Apagar(caminho, base) => Operacao::Apagar {
                caminho: caminho.clone(),
                base: *base,
            },
            Pendente::CriarPasta(caminho) => Operacao::CriarPasta {
                caminho: caminho.clone(),
            },
        };
        Some((op, operacao))
    }

    /// O canal caiu: o que estava em voo volta a esperar, e sai de novo quando ele voltar.
    ///
    /// Mandar de novo é seguro: a origem reconhece o mesmo conteúdo, a remoção do que já sumiu e a
    /// subpasta que já existe, e responde como aceitas.
    pub fn canal_caiu(&mut self) {
        for item in &mut self.fila {
            item.em_voo = None;
        }
    }

    /// Uma operação que estava em voo não pôde sair — o arquivo sumiu antes de ser lido, ou estava
    /// travado. Ela sai da fila; a varredura seguinte põe de volta o que ainda valer.
    pub fn desistir(&mut self, op: OpId) {
        let desistidas: Vec<String> = self
            .fila
            .iter()
            .filter(|f| f.em_voo == Some(op))
            .map(|f| f.pendente.caminho().to_owned())
            .collect();
        self.fila.retain(|f| f.em_voo != Some(op));
        // O índice esquece o que viu nesses caminhos: a varredura seguinte os vê como mudados e põe
        // de volta na fila o que ainda valer. Sem isto, um arquivo travado na hora de sair nunca iria.
        for caminho in desistidas {
            if let Some(local) = self.locais.get_mut(&caminho) {
                local.visto.modificado_ns = i64::MIN;
            }
        }
    }

    /// A resposta da origem a uma operação daqui. `resumo` é o do conteúdo que foi mandado, quando
    /// a operação era um envio.
    pub fn resultado(
        &mut self,
        op: OpId,
        resultado: &OpResult,
        resumo: Option<[u8; 32]>,
    ) -> Vec<Acao> {
        let Some(posicao) = self.fila.iter().position(|f| f.em_voo == Some(op)) else {
            return Vec::new();
        };
        let item = self.fila.remove(posicao);
        if let OpResult::Refused(motivo) = resultado {
            if *motivo == Refusal::Locked {
                // Travado do lado de lá: tenta de novo depois, no fim da fila.
                self.enfileirar(item.pendente);
            } else {
                self.recusadas = self.recusadas.saturating_add(1);
            }
            return Vec::new();
        }
        match (item.pendente, resultado) {
            (Pendente::Enviar(caminho), OpResult::Accepted { version }) => {
                self.confirmar_local(&caminho, *version, resumo);
                Vec::new()
            }
            // A decisão está nas mudanças que vêm a seguir; até lá, o conteúdo daqui é só "o daqui",
            // sem versão — que é o que deixa a leva seguinte movê-lo para a cópia de conflito.
            (Pendente::Enviar(caminho), OpResult::Conflict { .. }) => {
                self.confirmar_local(&caminho, 0, resumo);
                Vec::new()
            }
            (Pendente::CriarPasta(caminho), OpResult::Accepted { version }) => {
                self.confirmar_local(&caminho, *version, None);
                Vec::new()
            }
            // Apagado aqui o que a origem tinha mudado: a edição vence, e o arquivo volta.
            (Pendente::Apagar(caminho, _), OpResult::Resurrected { .. }) => {
                self.reavaliar(&caminho)
            }
            _ => Vec::new(),
        }
    }

    /// Um arquivo baixado chegou ao disco nesta versão. Devolve se valeu.
    ///
    /// Não vale se a origem já mudou de novo ou se o caminho ficou sujo enquanto baixava: quem
    /// executa confere com [`Self::pode_publicar`] antes de pôr o arquivo no lugar.
    pub fn baixado(&mut self, caminho: &str, versao: u64, visto: Visto) -> bool {
        let atual = self
            .remotas
            .get(caminho)
            .is_some_and(|e| e.version == versao);
        if !atual || self.sujo(caminho) {
            return false;
        }
        self.locais.insert(
            caminho.to_owned(),
            Local {
                visto,
                base: versao,
            },
        );
        true
    }

    /// Se um arquivo baixado pode ocupar o caminho agora, dado o que o disco mostra nele.
    ///
    /// Só se ninguém mexeu ali: nada no disco e nada no índice, ou o disco igual ao que o índice
    /// sabe. Um arquivo que o usuário mudou durante o download não é sobrescrito.
    #[must_use]
    pub fn pode_publicar(&self, caminho: &str, no_disco: Option<&Visto>) -> bool {
        if self.sujo(caminho) {
            return false;
        }
        match (self.locais.get(caminho), no_disco) {
            (None, None) => true,
            (Some(local), Some(visto)) => local.base > 0 && local.visto.igual_por_fora(visto),
            _ => false,
        }
    }

    /// Acerta o que o índice sabe de um caminho com o que o disco mostra depois de uma ação, sem
    /// mudar a versão: o horário que o sistema dá a um arquivo copiado não é o do original.
    pub fn registrar(&mut self, caminho: &str, visto: Visto) {
        if let Some(local) = self.locais.get_mut(caminho) {
            let resumo = local.visto.resumo;
            local.visto = visto;
            if visto.resumo.is_none() {
                local.visto.resumo = resumo;
            }
        }
    }

    /// A origem parou de compartilhar, ou esta réplica foi desligada dela: a fila não tem mais
    /// para onde ir.
    pub fn esquecer_fila(&mut self) {
        self.fila.clear();
    }

    fn enfileirar(&mut self, pendente: Pendente) {
        let repetida = self
            .fila
            .iter()
            .any(|f| f.em_voo.is_none() && f.pendente == pendente);
        if !repetida {
            self.fila.push(NaFila {
                pendente,
                em_voo: None,
            });
        }
    }

    fn confirmar_local(&mut self, caminho: &str, versao: u64, resumo: Option<[u8; 32]>) {
        if let Some(local) = self.locais.get_mut(caminho) {
            local.base = versao;
            if resumo.is_some() {
                local.visto.resumo = resumo;
            }
        }
    }

    /// A ação que põe uma entrada da origem no disco daqui.
    fn trazer(&mut self, entrada: &Entry) -> Acao {
        if entrada.kind == EntryKind::Directory {
            let visto = Visto::pasta(entrada.modified_ns);
            self.locais.insert(
                entrada.path.clone(),
                Local {
                    visto,
                    base: entrada.version,
                },
            );
            return Acao::CriarPasta(entrada.path.clone());
        }
        let baixar = Baixar {
            caminho: entrada.path.clone(),
            entrada: entrada.id,
            versao: entrada.version,
            tamanho: entrada.size,
            resumo: entrada.hash,
            modificado_ns: entrada.modified_ns,
        };
        if self.sob_demanda {
            Acao::Marcador(baixar)
        } else {
            Acao::Baixar(baixar)
        }
    }
}

#[cfg(test)]
mod testes;
