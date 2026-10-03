//! O que a varredura do disco daqui vira: mudanças pendentes, na ordem em que a origem precisa delas.

use super::{Local, Pendente, Replica};
use crate::ignorar::ignorar_caminho;
use crate::retrato::Retrato;

impl Replica {
    /// Aplica o que a varredura do disco daqui viu: o que mudou vira mudança pendente.
    ///
    /// A ordem da fila é a que a origem precisa: subpastas novas de cima para baixo, depois os
    /// arquivos, e as remoções de baixo para cima — o conteúdo sai antes da subpasta.
    pub fn aplicar_retrato(&mut self, retrato: &Retrato) {
        let mut pastas = Vec::new();
        let mut arquivos = Vec::new();
        let mut remocoes = Vec::new();
        for (caminho, visto) in retrato {
            if ignorar_caminho(caminho) {
                continue;
            }
            let anterior = self.locais.get(caminho).copied();
            let base = match anterior {
                Some(local) if local.visto.igual_por_fora(visto) => continue,
                // Arquivo virou subpasta, ou o contrário: o de antes foi apagado.
                Some(local) if local.visto.tipo != visto.tipo => {
                    if local.base > 0 {
                        remocoes.push(Pendente::Apagar(caminho.clone(), local.base));
                    }
                    0
                }
                Some(local) => local.base,
                None => 0,
            };
            let visto = *visto;
            self.locais.insert(caminho.clone(), Local { visto, base });
            if visto.eh_pasta() {
                pastas.push(Pendente::CriarPasta(caminho.clone()));
            } else {
                arquivos.push(Pendente::Enviar(caminho.clone()));
            }
        }
        let sumiram: Vec<(String, Local)> = self
            .locais
            .iter()
            .filter(|(caminho, _)| !retrato.contains_key(*caminho))
            .map(|(c, l)| (c.clone(), *l))
            .collect();
        for (caminho, local) in sumiram {
            self.locais.remove(&caminho);
            // Sai da fila o que mandaria o conteúdo ou criaria a subpasta — não há mais o que mandar.
            // Uma remoção anterior fica: apagado, recriado e apagado de novo, a origem ainda precisa
            // saber que a versão dela se foi.
            self.fila.retain(|f| {
                f.em_voo.is_some()
                    || f.pendente.caminho() != caminho
                    || matches!(f.pendente, Pendente::Apagar(..))
            });
            if local.base > 0 {
                remocoes.push(Pendente::Apagar(caminho, local.base));
            }
        }
        remocoes.sort_by(|a, b| b.caminho().cmp(a.caminho()));
        for pendente in pastas.into_iter().chain(arquivos).chain(remocoes) {
            self.enfileirar(pendente);
        }
    }
}
