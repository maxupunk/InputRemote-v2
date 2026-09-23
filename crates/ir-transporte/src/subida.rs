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
    /// O rádio que abrir depois: o canal estava ocupado, o adaptador não existia na subida, ou o
    /// rádio foi perdido e voltou ([`Reabridor`]).
    pub radio_tardio: UnboundedReceiver<RadioAberto>,
    /// Quem tenta abrir o rádio de novo quando ele é perdido.
    pub reabridor: Reabridor,
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
    let reabridor = Reabridor {
        identidade: Arc::clone(identidade),
        fatos: emissor,
        tardio,
        insistindo: Arc::default(),
    };
    let aberto = abrir_radio(&reabridor);
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
        reabridor,
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

/// Quantas vezes se tenta no ritmo rápido: 40 × 3 s = 2 min.
const TENTATIVAS: u32 = 40;

/// Depois do ritmo rápido, ou sem adaptador, de quanto em quanto tempo se tenta — para sempre.
///
/// Um adaptador USB espetado depois, o Bluetooth religado nas configurações: o rádio volta sozinho,
/// sem reiniciar o serviço. Devagar, porque abrir o rádio consulta o sistema, e ninguém nota 15 s.
const REABRIR_DEVAGAR: std::time::Duration = std::time::Duration::from_secs(15);

/// Quem tenta abrir o rádio de novo, em segundo plano, até conseguir.
///
/// Mora na subida porque é ela que tem a identidade e o canal de fatos. O ator só pede
/// ([`Reabridor::reabrir`]) quando o rádio é perdido, e o rádio que abrir chega a ele pelo mesmo
/// caminho do rádio tardio.
#[derive(Clone)]
pub struct Reabridor {
    identidade: Arc<ir_crypto::Identity>,
    fatos: UnboundedSender<Fato>,
    tardio: UnboundedSender<RadioAberto>,
    /// Se já há uma tentativa em curso: duas insistindo abririam dois rádios.
    insistindo: Arc<std::sync::atomic::AtomicBool>,
}

impl core::fmt::Debug for Reabridor {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("Reabridor")
    }
}

impl Reabridor {
    /// O rádio foi perdido: tenta abrir de novo, devagar, até conseguir.
    pub fn reabrir(&self) {
        self.insistir(0);
    }

    /// Tenta em segundo plano: `rapidas` vezes a cada 3 s, depois a cada 15 s, até abrir.
    fn insistir(&self, rapidas: u32) {
        use std::sync::atomic::Ordering;
        if self.insistindo.swap(true, Ordering::SeqCst) {
            return;
        }
        let eu = self.clone();
        tokio::spawn(async move {
            let mut tentativa = 0u32;
            loop {
                let espera = if tentativa < rapidas {
                    REABRIR_A_CADA
                } else {
                    REABRIR_DEVAGAR
                };
                tokio::time::sleep(espera).await;
                tentativa = tentativa.saturating_add(1);
                if eu.tardio.is_closed() {
                    break; // o ator saiu
                }
                if let Ok(aberto) = tentar_radio(&eu.identidade, &eu.fatos) {
                    let _ = eu.tardio.send(aberto);
                    break;
                }
            }
            eu.insistindo.store(false, Ordering::SeqCst);
        });
    }
}

/// Abre o rádio Bluetooth, se houver um — agora, ou em segundo plano.
///
/// **Não abrir não é falha do serviço.** Sem adaptador ou com ele desligado, o que resta é a rede,
/// e a política única do `ir-session` diz isso na tela. Mas **canal ocupado não é falta de rádio**:
/// numa atualização o serviço novo sobe segundos depois de o antigo parar, e o Windows ainda não
/// liberou o canal 23 (`os error 10048`, na bancada — log 44). Antes o serviço desistia do rádio até
/// a próxima reinicialização; agora tenta de novo em segundo plano, e o rádio que abrir chega ao ator
/// por `tardio`.
fn abrir_radio(reabridor: &Reabridor) -> Option<RadioAberto> {
    match tentar_radio(&reabridor.identidade, &reabridor.fatos) {
        Ok(aberto) => Some(aberto),
        Err(ir_bt::BtError::SemRadio(motivo)) => {
            info!(%motivo, "Bluetooth indisponível; a sessão vai usar a rede local, e o rádio entra se aparecer");
            reabridor.insistir(0);
            None
        }
        Err(erro) => {
            warn!(%erro, "o canal do Bluetooth ainda não abriu; tentando de novo em segundo plano");
            reabridor.insistir(TENTATIVAS);
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
