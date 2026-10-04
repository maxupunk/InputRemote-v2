//! Uma pasta compartilhada em uso: o índice guardado, o disco, e o que está atravessando agora.
//!
//! O motor (`ir-pasta`) decide; aqui se executa e se conversa. O que é de cada papel mora num
//! arquivo próprio — [`origem`](crate::viva::origem) e [`replica`](crate::viva::replica) —, e aqui
//! fica o que os dois fazem igual: varrer, guardar e resumir para a janela.

use std::collections::BTreeMap;
use std::path::PathBuf;

use ir_ipc::pastas::{ConflitoDePasta, IdDePasta, PapelDaPasta, ResumoDePasta, SituacaoDaPasta};
use ir_pasta::{Acao, Visto};
use ir_proto::message::{FolderId, FolderMessage, OpId};
use tracing::warn;

use crate::baixa::Baixas;
use crate::envio::{Envio, Recebimento};
use crate::guardado::{self, Guardada, Indice};
use crate::lugar::Lugar;
use crate::{Saida, varredura};

mod nuvem;
mod origem;
mod replica;

pub use nuvem::Repassar;
pub(crate) use nuvem::limpar_orfas;

/// O que a pasta precisa saber de fora para agir.
#[derive(Debug, Clone)]
pub struct Ambiente {
    /// Os lugares e a identidade deste computador.
    pub lugar: Lugar,
    /// Se o canal com o par está de pé e ele conhece pastas.
    pub conversando: bool,
    /// Se o canal está de pé, conheça ele pastas ou não.
    pub de_pe: bool,
    /// O nome do outro computador.
    pub nome_do_par: String,
    /// Quanto o relógio do par está adiantado em relação ao daqui.
    pub diferenca_ns: i64,
}

/// Uma pasta em uso.
#[derive(Debug)]
pub struct Viva {
    /// O que se guarda dela.
    pub guardada: Guardada,
    dir: PathBuf,
    /// Se mudou desde a última gravação.
    alterada: bool,
    /// Na réplica: o que está vindo da origem.
    baixas: Baixas,
    /// Na réplica: o arquivo indo para a origem.
    envio: Option<Envio>,
    /// Na réplica: uma leva de mudanças pedida, ou começada, que ainda não terminou. Sem isso, uma
    /// pasta grande dizia "em dia" entre uma mensagem e a seguinte da mesma leva.
    mudancas_por_vir: bool,
    /// Na origem: até onde a réplica já recebeu as mudanças, nesta conexão.
    enviado_ate: Option<u64>,
    /// Na origem: os arquivos chegando da réplica.
    recebendo: BTreeMap<OpId, Recebimento>,
    /// Quantos caminhos ficaram de fora pelo nome, na última varredura.
    invalidos: usize,
    /// Na réplica sob demanda do Windows: a raiz conectada e os pedidos de conteúdo em curso.
    nuvem: nuvem::Nuvem,
}

impl Viva {
    /// Uma pasta a partir do que estava guardado.
    #[must_use]
    pub fn de(guardada: Guardada, lugar: &Lugar) -> Self {
        let dir = lugar.da_pasta(guardada.pasta());
        Self {
            guardada,
            dir,
            alterada: false,
            baixas: Baixas::default(),
            envio: None,
            mudancas_por_vir: false,
            enviado_ate: None,
            recebendo: BTreeMap::new(),
            invalidos: 0,
            nuvem: nuvem::Nuvem::default(),
        }
    }

    /// A pasta.
    #[must_use]
    pub const fn pasta(&self) -> FolderId {
        self.guardada.pasta()
    }

    /// Se este computador compartilhou a pasta.
    #[must_use]
    pub const fn eh_origem(&self) -> bool {
        matches!(self.guardada.indice, Indice::Origem(_))
    }

    /// Onde ela fica neste computador.
    #[must_use]
    pub fn raiz(&self) -> &std::path::Path {
        &self.guardada.raiz
    }

    /// Onde a pessoa vê a pasta: o ponto de montagem, na sob demanda do Linux; a raiz, no resto.
    #[must_use]
    pub fn visivel(&self) -> &std::path::Path {
        self.guardada
            .ponto
            .as_deref()
            .unwrap_or(&self.guardada.raiz)
    }

    /// Varre o disco e leva o que mudou ao índice — e, na origem, à réplica.
    pub fn varrer(&mut self, ambiente: &Ambiente, saida: &mut dyn Saida) {
        let raiz = self.guardada.raiz.clone();
        let varrida = match &self.guardada.indice {
            Indice::Origem(origem) => {
                varredura::varrer(&raiz, &|c, v: &Visto| origem.precisa_de_resumo(c, v))
            }
            Indice::Replica(replica) => varredura::varrer(&raiz, &|c, v: &Visto| {
                replica
                    .local(c)
                    .is_none_or(|l| !l.visto.igual_por_fora(v) || l.visto.resumo.is_none())
            }),
        };
        let varrida = match varrida {
            Ok(varrida) => varrida,
            Err(erro) => {
                // Sem a raiz, um retrato vazio pareceria "apagaram tudo": melhor não fazer nada.
                warn!(%erro, "a pasta compartilhada não pôde ser lida; nada muda até ela voltar");
                return;
            }
        };
        self.invalidos = varrida.invalidos.len();
        // Um conflito cuja cópia sumiu foi resolvido — aqui ou do outro lado — e sai da lista.
        let antes = self.guardada.conflitos.len();
        self.guardada
            .conflitos
            .retain(|(_, copia)| varrida.retrato.contains_key(copia));
        self.alterada |= self.guardada.conflitos.len() != antes;
        match &mut self.guardada.indice {
            Indice::Origem(origem) => {
                if !origem.aplicar_retrato(&varrida.retrato).is_empty() {
                    self.alterada = true;
                    self.empurrar(ambiente, saida);
                }
            }
            Indice::Replica(replica) => {
                let antes = replica.pendentes();
                replica.aplicar_retrato(&varrida.retrato);
                self.alterada |= replica.pendentes() != antes;
                self.cuidar_dos_fixados();
            }
        }
    }

    /// Uma mensagem do par sobre esta pasta.
    pub fn do_par(&mut self, mensagem: FolderMessage, ambiente: &Ambiente, saida: &mut dyn Saida) {
        if self.eh_origem() {
            self.na_origem(mensagem, ambiente, saida);
        } else {
            self.na_replica(mensagem, ambiente, saida);
        }
    }

    /// O que andar sozinho: a próxima mudança daqui, o próximo download.
    pub fn andar(&mut self, ambiente: &Ambiente, saida: &mut dyn Saida) {
        if !ambiente.conversando || !self.guardada.aceita {
            return;
        }
        if self.eh_origem() {
            self.empurrar(ambiente, saida);
        } else {
            self.andar_replica(ambiente, saida);
        }
    }

    /// O canal caiu: o que estava atravessando volta a esperar.
    pub fn canal_caiu(&mut self) {
        self.enviado_ate = None;
        self.mudancas_por_vir = false;
        for (_, recebimento) in std::mem::take(&mut self.recebendo) {
            let _ = std::fs::remove_file(&recebimento.montado);
        }
        self.baixas.canal_caiu();
        self.soltar_buscas();
        if let Some(envio) = self.envio.take() {
            envio.descartar();
        }
        if let Indice::Replica(replica) = &mut self.guardada.indice {
            replica.canal_caiu();
        }
    }

    /// Grava o índice, se mudou.
    pub fn guardar(&mut self) {
        if !self.alterada {
            return;
        }
        match guardado::gravar(&self.dir, &self.guardada) {
            Ok(()) => self.alterada = false,
            Err(erro) => warn!(%erro, "não consegui gravar o índice da pasta; tento de novo"),
        }
    }

    /// Esquece a pasta: o índice sai, os arquivos ficam onde estão.
    pub fn esquecer(mut self) {
        self.canal_caiu();
        self.baixas.esvaziar();
        self.desligar_nuvem();
        guardado::esquecer(&self.dir);
    }

    /// Marca que o índice mudou.
    pub(crate) fn alterou(&mut self) {
        self.alterada = true;
    }

    /// A lixeira desta pasta: dentro dela na origem; fora, ao lado do índice, na réplica.
    pub(crate) fn lixeira(&self) -> PathBuf {
        if self.eh_origem() {
            self.guardada
                .raiz
                .join(ir_pasta::ignorar::PASTA_DE_CONTROLE)
                .join("lixeira")
        } else {
            self.dir.join("lixeira")
        }
    }

    /// Executa as ações que não pedem nada da rede. Devolve as que sobram — os downloads.
    pub(crate) fn executar(&mut self, acoes: Vec<Acao>) -> Vec<Acao> {
        let mut sobram = Vec::new();
        for acao in acoes {
            if let Err(erro) = self.executar_uma(&acao) {
                warn!(%erro, "uma ação de disco da pasta falhou; a próxima varredura acerta");
            }
            if matches!(acao, Acao::Baixar(_) | Acao::Marcador(_)) {
                sobram.push(acao);
            }
        }
        self.alterada = true;
        sobram
    }

    fn executar_uma(&mut self, acao: &Acao) -> std::io::Result<()> {
        let raiz = self.guardada.raiz.clone();
        let abs = |c: &str| crate::disco::absoluto(&raiz, c);
        // O que muda o que a pessoa vê passa pela montagem, no Linux; ler é no cache.
        let w = |caminho: &std::path::Path| self.pela_montagem(caminho);
        match acao {
            Acao::Copiar { de, para } => {
                crate::disco::copiar(&w(&raiz), &abs(de), &w(&abs(para)))?;
            }
            Acao::Mover { de, para } => crate::disco::mover(&w(&abs(de)), &w(&abs(para)))?,
            Acao::CriarPasta(c) => std::fs::create_dir_all(w(&abs(c)))?,
            Acao::ParaLixeira(c) => self.levar_a_lixeira(c)?,
            // Os que vêm da rede, e a publicação da origem, têm caminho próprio.
            Acao::Baixar(_) | Acao::Marcador(_) | Acao::Publicar(_) => return Ok(()),
        }
        self.registrar(acao.caminho());
        Ok(())
    }

    /// Acerta o índice com o que o disco mostra num caminho, depois de uma ação.
    pub(crate) fn registrar(&mut self, caminho: &str) {
        let Some(visto) =
            varredura::visto_de(&crate::disco::absoluto(&self.guardada.raiz, caminho))
        else {
            return;
        };
        match &mut self.guardada.indice {
            Indice::Origem(origem) => origem.acertar_com_o_disco(caminho, &visto),
            Indice::Replica(replica) => replica.registrar(caminho, visto),
        }
    }

    /// Como a janela mostra esta pasta.
    #[must_use]
    pub fn resumo(&self, ambiente: &Ambiente) -> ResumoDePasta {
        let (papel, pendentes, em_curso) = match &self.guardada.indice {
            Indice::Origem(origem) => (
                PapelDaPasta::Compartilhada,
                0,
                !self.recebendo.is_empty() || self.enviado_ate.is_some_and(|e| e < origem.seq()),
            ),
            Indice::Replica(replica) => (
                PapelDaPasta::Recebida,
                u32::try_from(replica.pendentes()).unwrap_or(u32::MAX),
                replica.pendentes() > 0 || self.baixas.pendentes() > 0 || self.mudancas_por_vir,
            ),
        };
        let situacao = if ambiente.de_pe && !ambiente.conversando {
            SituacaoDaPasta::ParDesatualizado
        } else if !ambiente.de_pe {
            SituacaoDaPasta::SemConexao
        } else if !self.guardada.aceita {
            SituacaoDaPasta::Oferecida
        } else if em_curso {
            SituacaoDaPasta::Sincronizando
        } else {
            SituacaoDaPasta::EmDia
        };
        ResumoDePasta {
            id: IdDePasta(self.pasta().0),
            nome: self.guardada.nome.clone(),
            caminho_local: self.visivel().to_string_lossy().into_owned(),
            papel,
            situacao,
            pendentes,
            conflitos: u32::try_from(self.guardada.conflitos.len()).unwrap_or(u32::MAX),
            baixando: u32::try_from(self.baixas.pendentes()).unwrap_or(u32::MAX),
            lista_de_conflitos: self
                .guardada
                .conflitos
                .iter()
                .map(|(original, copia)| ConflitoDePasta {
                    original: original.clone(),
                    copia: copia.clone(),
                })
                .collect(),
        }
    }

    /// Quantos caminhos ficaram de fora pelo nome.
    #[must_use]
    pub const fn invalidos(&self) -> usize {
        self.invalidos
    }
}
