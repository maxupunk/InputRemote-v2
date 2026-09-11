//! A bancada dos testes do ator: o serviço inteiro, sem rede, sem entrada e sem agente de verdade.
//!
//! O canal do agente fica com o teste, que confere o que o serviço mandou por ele. Um lugar só para
//! montar o serviço, para um campo novo em [`Parts`] não virar uma mudança em cada módulo de teste.

#![allow(clippy::expect_used)]

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use ir_ipc::{ComandoDoAgente, Maquina, Nome};
use ir_proto::ids::MachineId;
use ir_proto::peer::{Capabilities, MachineName};
use ir_proto::screens::Edge;
use ir_session::{LocalIdentity, Role};
use tokio::sync::{broadcast, mpsc};

use super::papel::texto_do_papel;
use super::{Daemon, Parts, nova_sessao};
use crate::config::Config;

/// Um diretório por teste, para dois testes não gravarem no mesmo arquivo.
static PROXIMO: AtomicUsize = AtomicUsize::new(0);

/// O serviço montado, e a ponta do canal por onde ele fala com o agente.
pub(super) struct Bancada {
    pub(super) daemon: Daemon,
    /// Onde este serviço grava o estado.
    pub(super) dir: PathBuf,
    /// O que o serviço mandou para o agente.
    pub(super) agente: broadcast::Receiver<ComandoDoAgente>,
}

impl Bancada {
    /// Um serviço no papel dado, com o par à direita e nenhum par gravado.
    pub(super) fn nova(papel: Role) -> Self {
        let dir = diretorio();
        let config = Config {
            role: texto_do_papel(papel).to_owned(),
            ..Config::default()
        };
        let (net, _) = mpsc::unbounded_channel();
        let (avisos, _) = broadcast::channel(16);
        let (agente, receptor_do_agente) = broadcast::channel(16);
        let daemon = Daemon::new(Parts {
            session: nova_sessao(papel, Edge::Right, identidade()),
            net,
            injector: None,
            capturer: None,
            screen: (1920, 1080),
            peer_addr: None,
            data_dir: dir.clone(),
            config,
            avisos,
            machine: Maquina([7; 16]),
            nome: Nome::coagido("bancada"),
            edge: Edge::Right,
            agente,
            identidade_local: identidade(),
        });
        Self {
            daemon,
            dir,
            agente: receptor_do_agente,
        }
    }
}

/// Um diretório de estado vazio e só deste teste.
pub(super) fn diretorio() -> PathBuf {
    let n = PROXIMO.fetch_add(1, Ordering::SeqCst);
    let dir = std::env::temp_dir().join(format!("ir-bancada-{}-{n}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("cria o diretório");
    dir
}

fn identidade() -> LocalIdentity {
    LocalIdentity {
        machine: MachineId([7; 16]),
        name: MachineName::coagido("bancada"),
        capabilities: Capabilities::default(),
    }
}
