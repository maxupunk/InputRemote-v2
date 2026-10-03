//! O que a réplica faz com uma leva de mudanças da origem.
//!
//! Duas passadas. A primeira procura, no disco daqui, conteúdo que já existe: um arquivo que a
//! origem renomeou, ou a versão daqui que perdeu um conflito e virou cópia ao lado. Mover ou copiar
//! localmente é instantâneo; baixar de novo o que já está no disco é só gastar rede. A segunda
//! passada faz o resto: criar, baixar, levar à lixeira.

use std::collections::BTreeSet;

use ir_proto::message::{Entry, EntryKind};

use super::{Local, Pendente, Replica};
use crate::acao::Acao;
use crate::retrato::dentro_de;

impl Replica {
    /// Aplica uma leva de mudanças da origem e devolve o que fazer no disco, na ordem.
    ///
    /// `up_to` é até onde a réplica fica completa depois desta leva (`FolderMessage::Changes`). Um
    /// caminho com mudança daqui pendente não é tocado: a origem decide quando a mudança chegar lá.
    pub fn aplicar_mudancas(&mut self, entradas: &[Entry], up_to: u64) -> Vec<Acao> {
        let acoes = self.aplicar(entradas);
        self.visto_ate = self.visto_ate.max(up_to);
        acoes
    }

    /// Confere de novo, contra o que a origem tem, um caminho e o que está abaixo dele.
    ///
    /// Para quando um caminho deixa de estar sujo sem a origem mandar nada novo sobre ele — a
    /// remoção daqui que ela recusou. As mudanças dele chegaram antes, enquanto ele estava sujo, e
    /// foram guardadas sem tocar o disco; agora valem, com o mesmo reaproveitamento de uma leva.
    pub(super) fn reavaliar(&mut self, caminho: &str) -> Vec<Acao> {
        let entradas: Vec<Entry> = self
            .remotas
            .iter()
            .filter(|(c, _)| c.as_str() == caminho || dentro_de(c, caminho))
            .map(|(_, e)| e.clone())
            .collect();
        self.aplicar(&entradas)
    }

    fn aplicar(&mut self, entradas: &[Entry]) -> Vec<Acao> {
        let mut acoes = Vec::new();
        let ocupados = self.reaproveitar(entradas, &mut acoes);
        for entrada in entradas {
            if ocupados.contains(&entrada.path) {
                continue;
            }
            if entrada.deleted {
                self.remotas.remove(&entrada.path);
                if !self.sujo(&entrada.path) {
                    self.levar_embora(&entrada.path, &mut acoes);
                }
                continue;
            }
            self.remotas.insert(entrada.path.clone(), entrada.clone());
            if !self.sujo(&entrada.path) {
                self.acompanhar(entrada, &mut acoes);
            }
        }
        acoes
    }

    /// A primeira passada: o conteúdo que chega e já está no disco daqui, noutro caminho.
    ///
    /// Devolve os caminhos que ela já resolveu.
    fn reaproveitar(&mut self, entradas: &[Entry], acoes: &mut Vec<Acao>) -> BTreeSet<String> {
        let de_saida = self.de_saida(entradas);
        let mut resolvidos = BTreeSet::new();
        for entrada in entradas {
            if entrada.deleted || entrada.kind != EntryKind::File || self.sujo(&entrada.path) {
                continue;
            }
            let Some(resumo) = entrada.hash else { continue };
            let ja_tem =
                self.locais.get(&entrada.path).and_then(|l| l.visto.resumo) == Some(resumo);
            if ja_tem {
                continue;
            }
            let Some((fonte, local)) = self.com_resumo(&entrada.path, resumo) else {
                continue;
            };
            // O destino tem de estar livre: vazio, ou com um conteúdo confirmado que esta leva
            // substitui. Ocupar um conteúdo daqui ainda não confirmado o perderia.
            let destino_livre = match self.locais.get(&entrada.path) {
                None => true,
                Some(l) => l.base > 0 && de_saida.contains(&entrada.path),
            };
            if !destino_livre {
                continue;
            }
            // Mover quando a fonte está de saída nesta leva, ou é a versão daqui que perdeu um
            // conflito (base zero, sem fila); copiar quando ela fica onde está.
            if de_saida.contains(&fonte) || local.base == 0 {
                self.mover_para(&fonte, &entrada.path, entradas, acoes);
            } else {
                acoes.push(Acao::Copiar {
                    de: fonte,
                    para: entrada.path.clone(),
                });
            }
            let visto = local.visto;
            let base = entrada.version;
            self.locais
                .insert(entrada.path.clone(), Local { visto, base });
            self.remotas.insert(entrada.path.clone(), entrada.clone());
            resolvidos.insert(entrada.path.clone());
        }
        resolvidos
    }

    /// Os caminhos cujo conteúdo daqui está de saída nesta leva — mudou ou foi apagado lá. É desses
    /// que um conteúdo pode ser **movido** sem perder nada.
    fn de_saida(&self, entradas: &[Entry]) -> BTreeSet<String> {
        entradas
            .iter()
            .filter(|e| {
                let daqui = self.locais.get(&e.path).and_then(|l| l.visto.resumo);
                e.deleted || (e.kind == EntryKind::File && daqui.is_some() && daqui != e.hash)
            })
            .map(|e| e.path.clone())
            .collect()
    }

    /// Move um conteúdo daqui para outro caminho, e o caminho vago recebe o que a origem tem nele
    /// se não vier nesta leva. É a versão daqui que perdeu um conflito: a da origem chegou numa leva
    /// anterior, quando o caminho ainda estava sujo, e não voltaria sozinha.
    fn mover_para(
        &mut self,
        fonte: &str,
        destino: &str,
        entradas: &[Entry],
        acoes: &mut Vec<Acao>,
    ) {
        self.locais.remove(fonte);
        acoes.push(Acao::Mover {
            de: fonte.to_owned(),
            para: destino.to_owned(),
        });
        let na_leva = entradas.iter().any(|e| e.path == fonte);
        if let Some(remota) = self.remotas.get(fonte).cloned().filter(|_| !na_leva) {
            let acao = self.trazer(&remota);
            acoes.push(acao);
        }
    }

    /// Um arquivo daqui, fora de `destino` e sem mudança pendente, com este conteúdo.
    fn com_resumo(&self, destino: &str, resumo: [u8; 32]) -> Option<(String, Local)> {
        self.locais
            .iter()
            .find(|(c, l)| c.as_str() != destino && l.visto.resumo == Some(resumo) && !self.sujo(c))
            .map(|(c, l)| (c.clone(), *l))
    }

    /// Uma entrada viva da origem: o disco daqui passa a ter o mesmo.
    fn acompanhar(&mut self, entrada: &Entry, acoes: &mut Vec<Acao>) {
        let local = self.locais.get(&entrada.path).copied();
        match (entrada.kind, local) {
            (EntryKind::Directory, Some(local)) if local.visto.eh_pasta() => {
                self.marcar_base(&entrada.path, entrada.version);
            }
            (EntryKind::File, Some(local))
                if local.visto.resumo.is_some() && local.visto.resumo == entrada.hash =>
            {
                self.marcar_base(&entrada.path, entrada.version);
            }
            // Um arquivo daqui onde lá há subpasta, ou o contrário: o daqui não some por isso.
            (EntryKind::Directory, Some(_)) => {}
            (EntryKind::File, Some(local)) if local.visto.eh_pasta() => {}
            (EntryKind::Directory, None) | (EntryKind::File, _) => acoes.push(self.trazer(entrada)),
        }
    }

    /// A origem apagou: o que está aqui vai para a lixeira — menos o que nasceu aqui e ela ainda
    /// não conhece, que fica.
    fn levar_embora(&mut self, caminho: &str, acoes: &mut Vec<Acao>) {
        let Some(local) = self.locais.get(caminho).copied() else {
            return;
        };
        if local.base == 0 {
            return;
        }
        if local.visto.eh_pasta() {
            let tem_novidade = self
                .locais
                .iter()
                .any(|(c, l)| dentro_de(c, caminho) && (l.base == 0 || self.sujo(c)));
            if tem_novidade {
                // O que nasceu aqui dentro precisa da subpasta: ela volta para a origem também.
                self.marcar_base(caminho, 0);
                self.fila.push(super::NaFila {
                    pendente: Pendente::CriarPasta(caminho.to_owned()),
                    em_voo: None,
                });
                return;
            }
            self.locais.retain(|c, _| !dentro_de(c, caminho));
        }
        self.locais.remove(caminho);
        acoes.push(Acao::ParaLixeira(caminho.to_owned()));
    }

    fn marcar_base(&mut self, caminho: &str, versao: u64) {
        if let Some(local) = self.locais.get_mut(caminho) {
            local.base = versao;
        }
    }
}
