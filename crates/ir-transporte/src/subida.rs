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
    /// O rádio que abrir depois, se o canal estava ocupado na subida ([`abrir_radio`]).
    pub radio_tardio: UnboundedReceiver<RadioAberto>,
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
    let (tardio, radio_tardio) = mpsc::unbounded_channel();
    let aberto = abrir_radio(Arc::clone(identidade), emissor, tardio);
    let descoberta = Descoberta::nova(maquina, aberto.as_ref().map(|a| a.pareados.clone()));
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
        radio_proprio: aberto.as_ref().and_then(|a| a.proprio),
        radio: aberto.map(|a| a.transporte),
        radio_tardio,
        fatos,
        descoberta,
    })
}

/// Um rádio aberto: o transporte, a alça dos pareados do sistema e o endereço dele.
pub struct RadioAberto {
    /// O transporte de rádio.
    pub transporte: Arc<dyn Transporte>,
    /// Para a busca listar os pareados do sistema.
    pub pareados: Pareados,
    /// O endereço deste rádio, quando ele diz.
    pub proprio: Option<ir_proto::ids::RadioAddress>,
}

impl core::fmt::Debug for RadioAberto {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("RadioAberto")
            .field("proprio", &self.proprio)
            .finish_non_exhaustive()
    }
}

/// De quanto em quanto tempo se tenta de novo um rádio cujo canal estava ocupado.
const REABRIR_A_CADA: std::time::Duration = std::time::Duration::from_secs(3);

/// Quantas vezes se tenta, antes de desistir: 40 × 3 s = 2 min.
const TENTATIVAS: u32 = 40;

/// Abre o rádio Bluetooth, se houver um — agora, ou em segundo plano.
///
/// **Não abrir não é falha do serviço.** Sem adaptador ou com ele desligado, o que resta é a rede,
/// e a política única do `ir-session` diz isso na tela. Mas **canal ocupado não é falta de rádio**:
/// numa atualização o serviço novo sobe segundos depois de o antigo parar, e o Windows ainda não
/// liberou o canal 23 (`os error 10048`, na bancada — log 44). Antes o serviço desistia do rádio até
/// a próxima reinicialização; agora tenta de novo em segundo plano, e o rádio que abrir chega ao ator
/// por `tardio`.
fn abrir_radio(
    identidade: Arc<ir_crypto::Identity>,
    fatos: UnboundedSender<Fato>,
    tardio: UnboundedSender<RadioAberto>,
) -> Option<RadioAberto> {
    match tentar_radio(&identidade, &fatos) {
        Ok(aberto) => Some(aberto),
        Err(ir_bt::BtError::SemRadio(motivo)) => {
            info!(%motivo, "Bluetooth indisponível; a sessão vai usar a rede local");
            None
        }
        Err(erro) => {
            info!(%erro, "o canal do Bluetooth ainda não abriu; tentando de novo em segundo plano");
            tokio::spawn(async move {
                for _ in 0..TENTATIVAS {
                    tokio::time::sleep(REABRIR_A_CADA).await;
                    if let Ok(aberto) = tentar_radio(&identidade, &fatos) {
                        let _ = tardio.send(aberto);
                        return;
                    }
                }
                warn!("o canal do Bluetooth não abriu; a sessão segue pela rede local");
            });
            None
        }
    }
}

/// Uma tentativa de abrir o rádio.
fn tentar_radio(
    identidade: &Arc<ir_crypto::Identity>,
    fatos: &UnboundedSender<Fato>,
) -> ir_bt::Result<RadioAberto> {
    let radio = Radio::abrir(Arc::clone(identidade), fatos.clone())?;
    let proprio = radio.endereco_proprio();
    if let Some(endereco) = proprio {
        info!(%endereco, "rádio Bluetooth aberto; junto com a rede, forma a rota dupla");
    } else {
        info!(
            "rádio Bluetooth aberto, sem endereço conhecido; o par só o alcança se já souber \
             para onde discar"
        );
    }
    let pareados = radio.pareados();
    Ok(RadioAberto {
        transporte: Arc::new(radio),
        pareados,
        proprio,
    })
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
