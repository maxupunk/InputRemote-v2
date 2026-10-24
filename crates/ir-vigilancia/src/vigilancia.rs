//! O estado do painel no serviço, e o que muda a cada segundo.

use std::sync::Arc;
use std::time::{Duration, Instant};

use ir_configuracao::{Config, Gravador};
use ir_crypto::{Identity, PublicKey};
use ir_ipc::Aviso;
use ir_monitor::{Mesa, Rota};
use ir_monitor_rede::Monitor;
use ir_proto::carrier::Carrier;
use ir_session::{Input, Notice};
use ir_telemetria::Vigia;

use crate::{Evento, Sessao};

/// Depois de quanto tempo sem renovação o pedido do par vale como desfeito. O mesmo prazo da
/// assinatura do painel ([`ir_monitor_rede::ASSINATURA_EXPIRA`]), pelo mesmo motivo.
const PEDIDO_DO_PAR_EXPIRA: Duration = ir_monitor_rede::ASSINATURA_EXPIRA;

/// A cada quantas batidas (de 1 s) o pedido ao par é renovado.
const RENOVAR_A_CADA: u32 = 2;

/// Por onde a vigilância devolve o que acontece fora do laço do serviço.
pub(crate) type Entregador = Arc<dyn Fn(Evento) -> bool + Send + Sync>;

/// O painel, visto pelo serviço.
pub struct Vigilancia {
    nome: String,
    identidade: Identity,
    entregar: Entregador,
    avisos: tokio::sync::broadcast::Sender<Aviso>,
    pub(crate) monitor: Option<Monitor>,
    mesa: Mesa,
    vigia: Option<Vigia>,
    pub(crate) assinantes: usize,
    /// Até quando o par quer a amostra daqui.
    par_quer_ate: Option<Instant>,
    /// Se este lado está pedindo a amostra do par.
    pedindo: bool,
    /// Se a mesa já sabe do par desta sessão.
    par_na_mesa: bool,
    pub(crate) conectados: Vec<PublicKey>,
    batidas: u32,
}

impl core::fmt::Debug for Vigilancia {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Vigilancia")
            .field("assinantes", &self.assinantes)
            .field("coletando", &self.vigia.is_some())
            .field("pedindo_ao_par", &self.pedindo)
            .finish_non_exhaustive()
    }
}

impl Vigilancia {
    /// A vigilância deste computador. `entregar` leva os [`Evento`]s ao laço do serviço e devolve
    /// se ele ainda os recebe; `avisos` é o emissor das janelas.
    pub fn nova(
        nome: &str,
        identidade: Identity,
        avisos: tokio::sync::broadcast::Sender<Aviso>,
        entregar: impl Fn(Evento) -> bool + Send + Sync + 'static,
    ) -> Self {
        Self {
            nome: nome.to_owned(),
            identidade,
            entregar: Arc::new(entregar),
            avisos,
            monitor: None,
            mesa: Mesa::nova(nome),
            vigia: None,
            assinantes: 0,
            par_quer_ate: None,
            pedindo: false,
            par_na_mesa: false,
            conectados: Vec::new(),
            batidas: 0,
        }
    }

    /// Um aviso da sessão. Só três interessam: a volta de cada rota, a amostra do par e o pedido
    /// dele.
    pub fn notar(&mut self, notice: &Notice) {
        let agora = Instant::now();
        match notice {
            Notice::RouteLatency { carrier, rtt } => {
                let rota = match carrier {
                    Carrier::Rfcomm => Rota::Bluetooth,
                    Carrier::Udp => Rota::Rede,
                    // O TCP não leva entrada, e a sessão não mede volta por ele.
                    Carrier::Tcp => return,
                };
                let micros = rtt.get().saturating_mul(1000);
                self.mesa.anotar_volta(rota, agora, micros);
            }
            Notice::PeerUsage(amostra) => self.mesa.anotar_par(agora, amostra.clone()),
            Notice::PeerWatchesUsage(quer) => {
                self.par_quer_ate = quer.then(|| agora + PEDIDO_DO_PAR_EXPIRA);
                self.ajustar_vigia();
            }
            _ => {}
        }
    }

    /// Algo que chegou de fora do laço: um aviso do monitor, ou a amostra deste segundo. Devolve
    /// as entradas para a sessão.
    #[must_use]
    pub fn evento(
        &mut self,
        evento: Evento,
        config: &mut Config,
        gravador: &Gravador,
    ) -> Vec<Input> {
        match evento {
            Evento::Monitor(aviso) => {
                self.do_monitor(aviso, config, gravador);
                Vec::new()
            }
            Evento::Amostra(amostra) => {
                self.mesa.anotar_local(amostra.clone());
                match self.par_quer_ate {
                    Some(_) => vec![Input::ShareUsage(amostra)],
                    None => Vec::new(),
                }
            }
        }
    }

    /// Conta às janelas.
    pub(crate) fn avisar(&self, aviso: Aviso) {
        let _ = self.avisos.send(aviso);
    }

    /// A batida de cada segundo: o monitor de pé, o par na mesa, o pedido ao par e o quadro.
    /// Devolve as entradas para a sessão.
    #[must_use]
    pub fn batida(&mut self, sessao: &impl Sessao, config: &Config) -> Vec<Input> {
        let agora = Instant::now();
        self.batidas = self.batidas.wrapping_add(1);
        self.garantir_monitor(config);
        self.acompanhar_o_par(sessao);
        if self.par_quer_ate.is_some_and(|ate| agora >= ate) {
            self.par_quer_ate = None;
        }
        self.ajustar_vigia();
        let entradas = self.pedir_ao_par(sessao);
        if self.assinantes > 0
            && let Some(monitor) = &self.monitor
        {
            monitor.publicar(self.mesa.quadro(agora));
        }
        entradas
    }

    /// Sobe o monitor na primeira batida com `tokio` por perto. Fora dele (os testes síncronos do
    /// serviço), não há porta a abrir.
    fn garantir_monitor(&mut self, config: &Config) {
        if self.monitor.is_some() || tokio::runtime::Handle::try_current().is_err() {
            return;
        }
        let (avisos, mut recebidos) = tokio::sync::mpsc::unbounded_channel();
        let entregar = Arc::clone(&self.entregar);
        tokio::spawn(async move {
            while let Some(aviso) = recebidos.recv().await {
                if !entregar(Evento::Monitor(aviso)) {
                    break;
                }
            }
        });
        self.monitor = Some(Monitor::iniciar(
            self.identidade.clone(),
            &self.nome,
            config.chaves_dos_paineis(),
            avisos,
        ));
    }

    /// A mesa segue a sessão: o par entra quando ela sobe, e sai — com as voltas dele — quando cai.
    fn acompanhar_o_par(&mut self, sessao: &impl Sessao) {
        match (sessao.par(), self.par_na_mesa) {
            (Some((nome, entende)), false) => {
                self.mesa.par_conectado(nome, entende);
                self.par_na_mesa = true;
            }
            (None, true) => {
                self.mesa.par_desconectado();
                self.par_na_mesa = false;
                // O pedido morreu com a sessão; a próxima recebe um novo.
                self.pedindo = false;
            }
            _ => {}
        }
    }

    /// Com painel assinando e par que entende, pede a amostra dele — renovando, porque ele a
    /// esquece sem renovação. Sem assinante, desfaz o pedido uma vez.
    fn pedir_ao_par(&mut self, sessao: &impl Sessao) -> Vec<Input> {
        let entende = sessao.par().is_some_and(|(_, entende)| entende);
        let quer = self.assinantes > 0 && entende;
        let renovar = !self.pedindo || self.batidas.is_multiple_of(RENOVAR_A_CADA);
        let desfazer = !quer && self.pedindo && entende;
        self.pedindo = quer;
        if quer && renovar {
            vec![Input::WatchPeerUsage(true)]
        } else if desfazer {
            vec![Input::WatchPeerUsage(false)]
        } else {
            Vec::new()
        }
    }

    /// A coleta só existe enquanto alguém a quer: um painel assinando, ou o par pedindo.
    pub(crate) fn ajustar_vigia(&mut self) {
        let precisa = self.assinantes > 0 || self.par_quer_ate.is_some();
        if precisa == self.vigia.is_some() {
            return;
        }
        if !precisa {
            self.vigia = None;
            tracing::info!("ninguém olhando: a coleta de uso parou");
            return;
        }
        let entregar = Arc::clone(&self.entregar);
        match Vigia::iniciar(move |amostra| entregar(Evento::Amostra(amostra))) {
            Ok(vigia) => {
                tracing::info!("um painel está olhando: a coleta de uso começou");
                self.vigia = Some(vigia);
            }
            Err(erro) => tracing::warn!(%erro, "a coleta de uso não começou"),
        }
    }

    /// O quadro de agora, como o painel o veria.
    #[must_use]
    pub fn quadro(&self) -> ir_monitor::Quadro {
        self.mesa.quadro(Instant::now())
    }

    /// Se a coleta está ligada agora.
    #[must_use]
    pub const fn coletando(&self) -> bool {
        self.vigia.is_some()
    }
}

#[cfg(test)]
mod tests;
