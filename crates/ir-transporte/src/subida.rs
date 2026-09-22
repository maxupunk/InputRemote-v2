//! Os portadores de entrada e a descoberta, abertos na subida do serviço.
//!
//! Saiu do `main` quando a descoberta entrou: abrir o rádio e anunciar-se na rede são a mesma
//! pergunta — por onde esta máquina alcança e é alcançada. Depois saiu do serviço para cá, quando
//! a rota dupla levou o `ir-daemon` além do teto de crate: a subida dos portadores é assunto da
//! fronteira dos portadores, e só usava tipos deste crate.

use std::sync::Arc;

use anyhow::Result;
use ir_proto::ids::MachineId;
use tokio::sync::mpsc::{self, UnboundedReceiver, UnboundedSender};
use tracing::{info, warn};

use crate::{Descoberta, Fato, Pareados, Radio, Rede, Transporte};

/// O que a subida entrega ao ator.
pub struct Abertos {
    /// O transporte de rede. Sempre existe.
    pub rede: Arc<dyn Transporte>,
    /// O rádio, quando há.
    pub radio: Option<Arc<dyn Transporte>>,
    /// O endereço do rádio desta máquina, quando há rádio e ele diz.
    pub radio_proprio: Option<ir_proto::ids::RadioAddress>,
    /// Os fatos dos dois transportes, num canal só.
    pub fatos: UnboundedReceiver<Fato>,
    /// A descoberta, já anunciando esta máquina na rede.
    pub descoberta: Descoberta,
}

impl core::fmt::Debug for Abertos {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Abertos")
            .field("radio", &self.radio.is_some())
            .field("radio_proprio", &self.radio_proprio)
            .finish_non_exhaustive()
    }
}

/// Sobe os dois transportes de entrada e a descoberta.
///
/// **Um canal de fatos só para os dois.** Cada fato já diz por onde veio, e é isso que permite
/// ao laço central ter um caminho de código para ambos os portadores — um canal por transporte
/// faria o laço crescer a cada portador novo.
///
/// # Errors
///
/// Só a rede: sem socket UDP não há serviço. Rádio e descoberta que não sobem viram uma linha no
/// registro, e o serviço segue.
pub async fn abrir(
    porta: u16,
    identidade: &Arc<ir_crypto::Identity>,
    maquina: MachineId,
) -> Result<Abertos> {
    let (emissor, fatos) = mpsc::unbounded_channel();
    let rede: Arc<dyn Transporte> =
        Arc::new(Rede::abrir(porta, Arc::clone(identidade), emissor.clone()).await?);
    let (radio, pareados, radio_proprio) = abrir_radio(Arc::clone(identidade), emissor);
    let descoberta = Descoberta::nova(maquina, pareados);
    match descoberta.anunciar(&nome_da_maquina(), porta).await {
        Ok(()) => info!(
            porta,
            "anunciado na rede local; os outros computadores já o encontram"
        ),
        Err(erro) => {
            warn!(%erro, "sem descoberta na rede; o par ainda pode ser digitado na janela");
        }
    }
    Ok(Abertos {
        rede,
        radio,
        radio_proprio,
        fatos,
        descoberta,
    })
}

/// Abre o rádio Bluetooth, se houver um, e a alça para listar os pareados do sistema.
///
/// **Não abrir não é falha do serviço.** Sem adaptador, com ele desligado, ou com o canal do
/// produto ocupado, o que resta é a rede — e é exatamente essa ausência que a política única do
/// `ir-session` transforma em "Bluetooth indisponível; usando a rede local", com o motivo
/// aparecendo na tela em vez de ficar escondido.
fn abrir_radio(
    identidade: Arc<ir_crypto::Identity>,
    fatos: UnboundedSender<Fato>,
) -> (
    Option<Arc<dyn Transporte>>,
    Option<Pareados>,
    Option<ir_proto::ids::RadioAddress>,
) {
    match Radio::abrir(identidade, fatos) {
        Ok(radio) => {
            let proprio = radio.endereco_proprio();
            if let Some(endereco) = proprio {
                info!(%endereco, "rádio Bluetooth aberto; junto com a rede, forma a rota dupla");
            } else {
                info!(
                    "rádio Bluetooth aberto, sem endereço conhecido; o par só o alcança se já                      souber para onde discar"
                );
            }
            let pareados = radio.pareados();
            (
                Some(Arc::new(radio) as Arc<dyn Transporte>),
                Some(pareados),
                proprio,
            )
        }
        Err(erro) => {
            info!(%erro, "Bluetooth indisponível; a sessão vai usar a rede local");
            if let Some(o_que_fazer) = erro.o_que_fazer() {
                info!("{o_que_fazer}");
            }
            (None, None, None)
        }
    }
}

/// O nome desta máquina, como o sistema a chama.
///
/// No Linux vem do núcleo, e não de `HOSTNAME`: essa variável é do shell, e um serviço do systemd
/// não a tem — o par aparecia do outro lado como "computador", e a descoberta anunciaria o mesmo.
pub fn nome_da_maquina() -> String {
    let do_sistema = if cfg!(windows) {
        std::env::var("COMPUTERNAME").ok()
    } else {
        std::fs::read_to_string("/proc/sys/kernel/hostname").ok()
    };
    do_sistema
        .map(|nome| nome.trim().to_owned())
        .filter(|nome| !nome.is_empty())
        .or_else(|| std::env::var("HOSTNAME").ok())
        .unwrap_or_else(|| "computador".to_owned())
}
