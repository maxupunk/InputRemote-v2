//! A bancada dos testes do ator: o serviço inteiro, sem rede, sem rádio, sem entrada e sem
//! agente de verdade.
//!
//! O transporte é de mentira e **anota o que lhe pediram**, que é como os testes verificam o que
//! o serviço mandou para o par. O canal do agente fica com o teste, pelo mesmo motivo. Um lugar
//! só para montar o serviço, para um campo novo em [`Parts`] não virar uma mudança em cada
//! módulo de teste.

#![allow(clippy::expect_used)]

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use ir_crypto::PublicKey;
use ir_ipc::{ComandoDoAgente, Maquina, Nome};
use ir_proto::carrier::Carrier;
use ir_proto::ids::MachineId;
use ir_proto::peer::{Capabilities, MachineName};
use ir_proto::screens::Edge;
use ir_session::{LocalIdentity, Role};
use tokio::sync::broadcast;

use super::papel::texto_do_papel;
use super::{Daemon, Parts, nova_sessao};
use crate::config::Config;
use ir_transporte::{Endereco, Transporte};

/// Um diretório por teste, para dois testes não gravarem no mesmo arquivo.
static PROXIMO: AtomicUsize = AtomicUsize::new(0);

/// O que o serviço pediu ao transporte.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Feito {
    /// Discou para este par. `fixada` diz se foi reconexão com a chave já gravada — o que
    /// distingue "reconectar" de "parear", e essa diferença é o assunto do log 25.
    Conectou {
        /// Para onde.
        alvo: Endereco,
        /// `true` em reconexão; `false` em primeiro pareamento.
        fixada: bool,
    },
    /// Mandou um quadro.
    Enviou(Vec<u8>),
    /// Respondeu à comparação de códigos.
    Confirmou(bool),
    /// Derrubou o enlace.
    Desconectou,
}

/// Um transporte que não fala com ninguém e anota tudo.
#[derive(Debug)]
pub(super) struct TransporteDeMentira {
    portador: Carrier,
    feitos: Mutex<Vec<Feito>>,
}

impl TransporteDeMentira {
    fn novo(portador: Carrier) -> Arc<Self> {
        Arc::new(Self {
            portador,
            feitos: Mutex::new(Vec::new()),
        })
    }

    /// O que foi pedido desde a última consulta.
    pub(super) fn feitos(&self) -> Vec<Feito> {
        let mut anotados = self.feitos.lock().expect("a bancada é de thread única");
        core::mem::take(&mut anotados)
    }

    fn anotar(&self, feito: Feito) {
        self.feitos
            .lock()
            .expect("a bancada é de thread única")
            .push(feito);
    }
}

impl Transporte for TransporteDeMentira {
    fn portador(&self) -> Carrier {
        self.portador
    }

    fn conectar(&self, alvo: Endereco, chave: Option<PublicKey>) {
        self.anotar(Feito::Conectou {
            alvo,
            fixada: chave.is_some(),
        });
    }

    fn enviar(&self, bytes: Vec<u8>) {
        self.anotar(Feito::Enviou(bytes));
    }

    fn confirmar_pareamento(&self, conferiu: bool) {
        self.anotar(Feito::Confirmou(conferiu));
    }

    fn desconectar(&self) {
        self.anotar(Feito::Desconectou);
    }
}

/// O serviço montado, com as duas pontas por onde os testes o observam.
pub(super) struct Bancada {
    pub(super) daemon: Daemon,
    /// Onde este serviço grava o estado. Apagado junto com a bancada.
    pub(super) dir: Diretorio,
    /// O que o serviço mandou para o agente.
    pub(super) agente: broadcast::Receiver<ComandoDoAgente>,
    /// O transporte de rede, para conferir o que foi pedido a ele.
    pub(super) rede: Arc<TransporteDeMentira>,
    /// O transporte de rádio, presente nesta bancada para o Bluetooth ser testável sem rádio.
    pub(super) radio: Arc<TransporteDeMentira>,
}

impl Bancada {
    /// Um serviço no papel dado, com o par à direita e nenhum par gravado.
    pub(super) fn nova(papel: Role) -> Self {
        let dir = diretorio();
        let config = Config {
            role: texto_do_papel(papel).to_owned(),
            ..Config::default()
        };
        let rede = TransporteDeMentira::novo(Carrier::Udp);
        let radio = TransporteDeMentira::novo(Carrier::Rfcomm);
        let (avisos, _) = broadcast::channel(16);
        let (agente, receptor_do_agente) = broadcast::channel(16);
        // A busca na rede não roda na bancada (não há runtime nos testes síncronos): o receptor
        // morre aqui, e um achado mandado para ele só se perde.
        let (de_fundo, _) = tokio::sync::mpsc::unbounded_channel();
        let daemon = Daemon::new(Parts {
            // A bancada exercita o ator, e o canal de arquivos não faz parte dele.
            arquivos: ir_transferencia::Pedidos::desligada(),
            descoberta: ir_transporte::Descoberta::desligada(),
            session: nova_sessao(papel, Edge::Right, identidade()),
            rede: Arc::clone(&rede) as Arc<dyn Transporte>,
            radio: Some(Arc::clone(&radio) as Arc<dyn Transporte>),
            injector: None,
            capturer: None,
            screen: (1920, 1080),
            peer: None,
            data_dir: dir.clone(),
            config,
            avisos,
            machine: Maquina([7; 16]),
            nome: Nome::coagido("bancada"),
            edge: Edge::Right,
            agente,
            identidade_local: identidade(),
            ajudantes: crate::ipc::Ajudantes::default(),
            radio_proprio: None,
            de_fundo,
        });
        Self {
            daemon,
            dir,
            agente: receptor_do_agente,
            rede,
            radio,
        }
    }

    /// Se o serviço discou para alguém desde a última consulta.
    pub(super) fn discou(feitos: &[Feito]) -> bool {
        feitos
            .iter()
            .any(|feito| matches!(feito, Feito::Conectou { .. }))
    }
}

/// Um diretório de estado vazio e só deste teste, apagado quando o teste termina.
///
/// Apagar não é capricho: sem isso cada `cargo test` deixava vinte pastas no TEMP, e a máquina de
/// desenvolvimento chegou a ter mais de seiscentas.
pub(super) fn diretorio() -> Diretorio {
    let n = PROXIMO.fetch_add(1, Ordering::SeqCst);
    let dir = std::env::temp_dir().join(format!("ir-bancada-{}-{n}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("cria o diretório");
    Diretorio(dir)
}

/// Um diretório de teste que se apaga ao sair de escopo, inclusive quando o teste falha.
#[derive(Debug)]
pub(super) struct Diretorio(PathBuf);

impl std::ops::Deref for Diretorio {
    type Target = PathBuf;

    fn deref(&self) -> &PathBuf {
        &self.0
    }
}

impl Drop for Diretorio {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn identidade() -> LocalIdentity {
    LocalIdentity {
        machine: MachineId([7; 16]),
        name: MachineName::coagido("bancada"),
        capabilities: Capabilities::default(),
    }
}
