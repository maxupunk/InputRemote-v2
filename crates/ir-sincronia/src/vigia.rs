//! A vigia do disco: o sistema avisa quando algo muda dentro de uma pasta compartilhada.
//!
//! O aviso não diz o que mudou de um jeito em que se possa confiar — o Windows junta eventos, o
//! `inotify` estoura a fila em pastas grandes —, então ele só **marca a pasta para varrer**. Quem
//! decide o que mudou é a varredura, depois de um intervalo de silêncio: um "salvar" do Office é
//! meia dúzia de eventos (temporário, renomear, apagar) que viram uma varredura só. E uma
//! varredura completa de tempos em tempos cobre o aviso que se perdeu.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::mpsc::Sender;

use ir_proto::message::FolderId;
use notify::{RecursiveMode, Watcher};
use tracing::{debug, warn};

use crate::laco::Evento;

/// Quem vigia cada pasta.
pub struct Vigia {
    observador: notify::RecommendedWatcher,
    raizes: BTreeMap<FolderId, PathBuf>,
}

impl std::fmt::Debug for Vigia {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Vigia")
            .field("raizes", &self.raizes)
            .finish_non_exhaustive()
    }
}

impl Vigia {
    /// Uma vigia que manda `Evento::Mudou` por `eventos`.
    ///
    /// # Errors
    ///
    /// Quando o sistema não oferece aviso de mudança — a varredura periódica segue cobrindo.
    pub fn nova(eventos: Sender<Evento>) -> notify::Result<Self> {
        let observador =
            notify::recommended_watcher(move |resultado: notify::Result<notify::Event>| {
                repassar(resultado, &eventos);
            })?;
        Ok(Self {
            observador,
            raizes: BTreeMap::new(),
        })
    }

    /// Passa a vigiar exatamente estas pastas.
    pub fn acompanhar(&mut self, raizes: &[(FolderId, PathBuf)]) {
        let novas: BTreeMap<FolderId, PathBuf> = raizes.iter().cloned().collect();
        for (pasta, raiz) in &self.raizes {
            if novas.get(pasta) != Some(raiz) {
                let _ = self.observador.unwatch(raiz);
            }
        }
        for (pasta, raiz) in &novas {
            if self.raizes.get(pasta) != Some(raiz)
                && let Err(erro) = self.observador.watch(raiz, RecursiveMode::Recursive)
            {
                warn!(%erro, "não consegui vigiar uma pasta; ela é varrida de tempos em tempos");
            }
        }
        self.raizes = novas;
    }
}

/// Leva ao laço o que o sistema avisou.
fn repassar(resultado: notify::Result<notify::Event>, eventos: &Sender<Evento>) {
    match resultado {
        Ok(evento) => {
            for caminho in evento.paths.into_iter().filter(|c| !da_sincronia(c)) {
                let _ = eventos.send(Evento::Mudou(caminho));
            }
        }
        // Fila estourada, e afins: varrer tudo é o que cobre.
        Err(erro) => {
            debug!(%erro, "a vigia perdeu eventos; varrer tudo");
            let _ = eventos.send(Evento::VarrerTudo);
        }
    }
}

/// Se o caminho é da própria sincronia — a montagem e a lixeira —, que não é mudança do usuário.
fn da_sincronia(caminho: &std::path::Path) -> bool {
    caminho
        .components()
        .any(|c| c.as_os_str() == ir_pasta::ignorar::PASTA_DE_CONTROLE)
}
