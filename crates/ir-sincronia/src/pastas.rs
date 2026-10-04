//! Todas as pastas compartilhadas deste usuário, e a conversa de sessão com o par.
//!
//! Quando o canal sobe, cada lado diz que pastas conhece e em que papel (`Hello`). A partir daí:
//!
//! - a origem oferece o que ainda não foi aceito;
//! - a réplica pede as mudanças desde o último número que aplicou — o que perdeu offline chega
//!   agora, e o que mudou aqui offline sai da fila;
//! - uma pasta que um lado conhece e o outro não deixou de ser compartilhada enquanto o canal estava
//!   fora: este lado também esquece. **Os arquivos ficam onde estão** — esquecer é só parar de
//!   sincronizar.

use std::collections::BTreeMap;
use std::path::PathBuf;

use ir_ipc::pastas::{IdDePasta, PapelDaPasta, ResumoDePasta, SituacaoDaPasta};
use ir_proto::limits::MAX_KNOWN_FOLDERS;
use ir_proto::message::{FolderId, FolderMessage, KnownFolder, Role};
use tracing::info;

use crate::guardado::{self, Indice};
use crate::lugar::Lugar;
use crate::viva::{Ambiente, Repassar, Viva};
use crate::{Saida, atalho};

mod comandos;
mod copia;

/// Uma pasta que o outro computador ofereceu e espera resposta.
#[derive(Debug, Clone)]
pub(crate) struct Oferta {
    pub(crate) nome: String,
}

/// As pastas deste usuário.
pub struct Pastas {
    lugar: Lugar,
    vivas: BTreeMap<FolderId, Viva>,
    ofertas: BTreeMap<FolderId, Oferta>,
    de_pe: bool,
    par_suporta: bool,
    /// Se o ajudante do outro lado respondeu nesta conexão — sem ele, não há com quem conversar.
    par_presente: bool,
    nome_do_par: String,
    diferenca_ns: i64,
    /// Para onde vão os pedidos de conteúdo do Windows, quando há réplica sob demanda.
    repassar: Option<Repassar>,
    /// O que o outro computador copiou de uma pasta, já com os caminhos daqui, para o clipboard.
    para_o_clipboard: Option<Vec<String>>,
}

impl std::fmt::Debug for Pastas {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Pastas")
            .field("vivas", &self.vivas.len())
            .field("ofertas", &self.ofertas.len())
            .finish_non_exhaustive()
    }
}

impl Pastas {
    /// As pastas guardadas deste usuário.
    #[must_use]
    pub fn abrir(lugar: Lugar) -> Self {
        let mut vivas = BTreeMap::new();
        for guardada in guardado::todas(&lugar.estado) {
            let viva = Viva::de(guardada, &lugar);
            crate::disco::faxina(&viva.lixeira());
            vivas.insert(viva.pasta(), viva);
        }
        info!(pastas = vivas.len(), "pastas compartilhadas carregadas");
        Self {
            lugar,
            vivas,
            ofertas: BTreeMap::new(),
            de_pe: false,
            par_suporta: false,
            par_presente: false,
            nome_do_par: String::new(),
            diferenca_ns: 0,
            repassar: None,
            para_o_clipboard: None,
        }
    }

    /// Liga as réplicas sob demanda ao Windows: daqui em diante, abrir um arquivo que ainda não veio
    /// chama `repassar`, e quem a recebe devolve o pedido em [`Self::da_nuvem`].
    pub fn usar_nuvem(&mut self, repassar: Repassar) {
        let manter: Vec<String> = self
            .vivas
            .values()
            .filter_map(Viva::raiz_sob_demanda)
            .collect();
        crate::viva::limpar_orfas(&manter);
        let ambiente = self.ambiente();
        for viva in self.vivas.values_mut() {
            viva.ligar_nuvem(&ambiente, &repassar);
        }
        self.repassar = Some(repassar);
    }

    /// Desliga do Windows toda pasta sob demanda deste usuário, para a desinstalação: o que veio
    /// fica como arquivo comum, o que não veio sai — está inteiro no outro computador —, a raiz sai
    /// do painel do Explorer, e a pasta deixa de ser sincronizada. Devolve quantas.
    ///
    /// Esquecer a pasta não é detalhe: sem o índice, a próxima varredura veria os arquivos que
    /// saíram como apagados aqui, e os apagaria no outro computador.
    pub fn desligar_sob_demanda(&mut self) -> usize {
        let sob_demanda: Vec<FolderId> = self
            .vivas
            .values()
            .filter(|v| v.raiz_sob_demanda().is_some())
            .map(Viva::pasta)
            .collect();
        for pasta in &sob_demanda {
            self.esquecer(*pasta);
        }
        crate::viva::limpar_orfas(&[]);
        sob_demanda.len()
    }

    /// Um pedido do Windows para uma réplica sob demanda.
    pub fn da_nuvem(&mut self, pedido: ir_nuvem::Pedido, saida: &mut dyn Saida) {
        let ambiente = self.ambiente();
        match pedido {
            ir_nuvem::Pedido::Buscar(busca) => {
                let viva = self.vivas.values_mut().find(|v| v.visivel() == busca.raiz);
                if let Some(viva) = viva {
                    viva.buscar(busca, &ambiente, saida);
                }
            }
            ir_nuvem::Pedido::Cancelar { transferencia } => {
                for viva in self.vivas.values_mut() {
                    viva.cancelar_busca(transferencia, saida);
                }
            }
        }
    }

    /// Os identificadores, para o serviço saber de quem são.
    #[must_use]
    pub fn ids(&self) -> Vec<IdDePasta> {
        self.vivas.keys().map(|p| IdDePasta(p.0)).collect()
    }

    /// Onde cada pasta fica, para a vigia.
    #[must_use]
    pub fn raizes(&self) -> Vec<(FolderId, PathBuf)> {
        self.vivas
            .iter()
            .map(|(p, v)| (*p, v.raiz().to_path_buf()))
            .collect()
    }

    pub(crate) fn ambiente(&self) -> Ambiente {
        Ambiente {
            lugar: self.lugar.clone(),
            conversando: self.de_pe && self.par_suporta && self.par_presente,
            de_pe: self.de_pe,
            nome_do_par: self.nome_do_par.clone(),
            diferenca_ns: self.diferenca_ns,
        }
    }

    /// O canal com o par mudou.
    pub fn enlace(
        &mut self,
        de_pe: bool,
        par_suporta: bool,
        nome_do_par: String,
        saida: &mut dyn Saida,
    ) {
        let subiu = de_pe && par_suporta && !(self.de_pe && self.par_suporta);
        if !(de_pe && par_suporta) {
            self.par_presente = false;
            for viva in self.vivas.values_mut() {
                viva.canal_caiu();
            }
        }
        self.de_pe = de_pe;
        self.par_suporta = par_suporta;
        self.nome_do_par = nome_do_par;
        if subiu {
            self.dizer_ola(false, saida);
        }
    }

    fn dizer_ola(&self, reply: bool, saida: &mut dyn Saida) {
        let pastas = self
            .vivas
            .values()
            .take(MAX_KNOWN_FOLDERS)
            .map(|viva| match &viva.guardada.indice {
                Indice::Origem(origem) => KnownFolder {
                    folder: viva.pasta(),
                    role: Role::Origin,
                    seen_up_to: origem.seq(),
                },
                Indice::Replica(replica) => KnownFolder {
                    folder: viva.pasta(),
                    role: Role::Replica,
                    seen_up_to: replica.visto_ate(),
                },
            })
            .collect();
        saida.enviar(FolderMessage::Hello {
            folders: pastas,
            clock_ns: crate::disco::nanos_agora(),
            reply,
        });
    }

    /// Uma mensagem do par.
    pub fn do_par(&mut self, mensagem: FolderMessage, saida: &mut dyn Saida) {
        match mensagem {
            FolderMessage::Hello {
                folders,
                clock_ns,
                reply,
            } => {
                if !reply {
                    self.dizer_ola(true, saida);
                }
                self.ola(&folders, clock_ns, saida);
            }
            FolderMessage::HelperAbsent => {
                self.par_presente = false;
                for viva in self.vivas.values_mut() {
                    viva.canal_caiu();
                }
            }
            FolderMessage::Offer { folder, name, .. } => {
                let nova = !self.vivas.contains_key(&folder) && !self.ofertas.contains_key(&folder);
                if nova {
                    crate::atalho::avisar_oferta(&self.nome_do_par, &name);
                }
                if !self.vivas.contains_key(&folder) {
                    self.ofertas.insert(folder, Oferta { nome: name });
                }
            }
            FolderMessage::Copied { folder, paths } => self.copiado_la(folder, &paths),
            FolderMessage::Stop { folder } | FolderMessage::Decline { folder, .. } => {
                self.ofertas.remove(&folder);
                self.esquecer(folder);
            }
            outra => {
                let ambiente = self.ambiente();
                if let Some(viva) = outra.folder().and_then(|p| self.vivas.get_mut(&p)) {
                    viva.do_par(outra, &ambiente, saida);
                }
            }
        }
    }

    fn ola(&mut self, folders: &[KnownFolder], clock_ns: i64, saida: &mut dyn Saida) {
        self.par_presente = true;
        self.diferenca_ns = clock_ns.saturating_sub(crate::disco::nanos_agora());
        let do_par: BTreeMap<FolderId, Role> = folders.iter().map(|k| (k.folder, k.role)).collect();
        let esquecidas: Vec<FolderId> = self
            .vivas
            .values()
            .filter(|viva| {
                let par = do_par.get(&viva.pasta());
                if viva.eh_origem() {
                    viva.guardada.aceita && par.is_none()
                } else {
                    par != Some(&Role::Origin)
                }
            })
            .map(Viva::pasta)
            .collect();
        for pasta in esquecidas {
            info!("o outro computador não conhece mais esta pasta; ela deixa de ser sincronizada");
            self.esquecer(pasta);
        }
        let ambiente = self.ambiente();
        for viva in self.vivas.values_mut() {
            viva.canal_caiu();
            match &viva.guardada.indice {
                Indice::Origem(origem) if !viva.guardada.aceita => {
                    let (entries, total_bytes) = origem.tamanho();
                    saida.enviar(FolderMessage::Offer {
                        folder: viva.pasta(),
                        name: viva.guardada.nome.clone(),
                        entries,
                        total_bytes,
                    });
                }
                Indice::Replica(_) => viva.pedir_mudancas(saida),
                Indice::Origem(_) => {}
            }
            viva.andar(&ambiente, saida);
        }
    }

    /// Varre uma pasta, ou todas.
    pub fn varrer(&mut self, so: Option<FolderId>, saida: &mut dyn Saida) {
        let ambiente = self.ambiente();
        for (pasta, viva) in &mut self.vivas {
            if so.is_none_or(|so| so == *pasta) {
                viva.varrer(&ambiente, saida);
            }
        }
    }

    /// Anda o que anda sozinho, e guarda o que mudou.
    pub fn andar(&mut self, saida: &mut dyn Saida) {
        let ambiente = self.ambiente();
        for viva in self.vivas.values_mut() {
            viva.avisar_alcance(ambiente.conversando);
            viva.andar(&ambiente, saida);
            viva.guardar();
        }
    }

    /// O que a janela mostra: as pastas e as ofertas.
    #[must_use]
    pub fn resumo(&self) -> Vec<ResumoDePasta> {
        let ambiente = self.ambiente();
        let mut resumo: Vec<ResumoDePasta> =
            self.vivas.values().map(|v| v.resumo(&ambiente)).collect();
        resumo.extend(self.ofertas.iter().map(|(pasta, oferta)| ResumoDePasta {
            id: IdDePasta(pasta.0),
            nome: oferta.nome.clone(),
            caminho_local: String::new(),
            papel: PapelDaPasta::Recebida,
            situacao: SituacaoDaPasta::Oferecida,
            pendentes: 0,
            conflitos: 0,
            baixando: 0,
            lista_de_conflitos: Vec::new(),
            lixeira_com_algo: false,
        }));
        resumo
    }

    /// Para de sincronizar uma pasta; os arquivos ficam.
    fn esquecer(&mut self, pasta: FolderId) {
        if let Some(viva) = self.vivas.remove(&pasta) {
            if !viva.eh_origem() {
                atalho::tirar(viva.visivel());
            }
            viva.esquecer();
        }
    }
}
