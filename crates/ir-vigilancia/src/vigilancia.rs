//! O estado do painel no serviço, e o que muda a cada segundo.

use std::sync::Arc;
use std::time::{Duration, Instant};

use ir_configuracao::{Config, Gravador};
use ir_crypto::{Identity, PublicKey};
use ir_ipc::Aviso;
use ir_ipc::transferencia::Velocimetro;
use ir_monitor::amostra::UsageSample;
use ir_monitor::{InteresseEmProcessos, Mesa, Processos, Rota};
use ir_monitor_rede::Monitor;
use ir_proto::carrier::Carrier;
use ir_session::{Input, Notice};
use ir_telemetria::Vigia;

use crate::{Evento, Sessao};

mod copia;
mod par;

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
    /// O que a vigia mede dos processos agora.
    na_vigia: InteresseEmProcessos,
    pub(crate) assinantes: usize,
    /// O que os painéis daqui querem dos processos: deste computador (0) e do par (1).
    pub(crate) dos_paineis: [InteresseEmProcessos; 2],
    /// Até quando o par quer a amostra daqui, e com que processos.
    par_quer: Option<(Instant, InteresseEmProcessos)>,
    /// O último pedido feito ao par; `None` quando este lado não está pedindo.
    pedido: Option<InteresseEmProcessos>,
    /// Se a mesa já sabe do par desta sessão.
    par_na_mesa: bool,
    pub(crate) conectados: Vec<PublicKey>,
    batidas: u32,
    /// A velocidade da cópia em curso, e de qual cópia.
    velocimetro: (Velocimetro, Option<String>),
}

impl core::fmt::Debug for Vigilancia {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Vigilancia")
            .field("assinantes", &self.assinantes)
            .field("coletando", &self.vigia.is_some())
            .field("pedindo_ao_par", &self.pedido.is_some())
            .finish_non_exhaustive()
    }
}

impl Vigilancia {
    /// A vigilância deste computador. `entregar` leva os [`Evento`]s ao laço do serviço e devolve
    /// se ele ainda os recebe; `avisos` é o emissor das janelas — de onde vem, também, a cópia em
    /// curso.
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
            na_vigia: InteresseEmProcessos::default(),
            assinantes: 0,
            dos_paineis: Default::default(),
            par_quer: None,
            pedido: None,
            par_na_mesa: false,
            conectados: Vec::new(),
            batidas: 0,
            velocimetro: (Velocimetro::default(), None),
        }
    }

    /// Um aviso da sessão. Só dois interessam: a volta de cada rota e o recado do painel do par.
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
            Notice::PeerPanel(recado) => self.recado_do_par(agora, recado),
            _ => {}
        }
    }

    /// Algo que chegou de fora do laço: um aviso do monitor, a amostra deste segundo ou a cópia
    /// em curso. Devolve as entradas para a sessão.
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
            Evento::Amostra(amostra, processos) => self.amostra(amostra, processos.as_ref()),
            Evento::Copia(transferencia) => {
                self.copia(&transferencia);
                Vec::new()
            }
        }
    }

    /// A amostra deste segundo: na mesa, com os processos que os painéis daqui pediram, e para o
    /// par, com os que o painel dele pediu.
    fn amostra(&mut self, amostra: UsageSample, processos: Option<&Processos>) -> Vec<Input> {
        let [daqui, _] = &self.dos_paineis;
        self.mesa
            .anotar_processos(processos.and_then(|p| p.para(daqui)));
        self.mesa.anotar_local(amostra.clone());
        self.par_quer
            .as_ref()
            .and_then(|(_, interesse)| {
                let processos = processos.and_then(|p| p.para(interesse));
                par::mandar(&ir_monitor::EntrePcs::Uso(amostra, processos))
            })
            .into_iter()
            .collect()
    }

    /// Conta às janelas.
    pub(crate) fn avisar(&self, aviso: Aviso) {
        let _ = self.avisos.send(aviso);
    }

    /// A batida de cada segundo: o monitor de pé, o par na mesa, o pedido ao par, o quadro e os
    /// detalhes. Devolve as entradas para a sessão.
    #[must_use]
    pub fn batida(&mut self, sessao: &impl Sessao, config: &Config) -> Vec<Input> {
        let agora = Instant::now();
        self.batidas = self.batidas.wrapping_add(1);
        self.garantir_monitor(config);
        self.acompanhar_o_par(sessao);
        if self.par_quer.as_ref().is_some_and(|(ate, _)| agora >= *ate) {
            self.par_quer = None;
        }
        self.ajustar_vigia();
        let entradas = self.pedir_ao_par(sessao);
        if self.assinantes > 0
            && let Some(monitor) = &self.monitor
        {
            monitor.publicar(self.mesa.quadro(agora), self.mesa.detalhes(agora));
        }
        entradas
    }

    /// Sobe o monitor na primeira batida com `tokio` por perto, e passa a ouvir as cópias. Fora
    /// dele (os testes síncronos do serviço), não há porta a abrir.
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
        copia::ouvir(self.avisos.subscribe(), Arc::clone(&self.entregar));
        let monitor = Monitor::iniciar(
            self.identidade.clone(),
            &self.nome,
            config.chaves_dos_paineis(),
            avisos,
        );
        monitor.comandar(ir_monitor_rede::Comando::AceitarPedidos(
            config.painel_aceita_pedidos,
        ));
        self.monitor = Some(monitor);
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
                self.pedido = None;
            }
            _ => {}
        }
    }

    /// A coleta só existe enquanto alguém a quer: um painel assinando, ou o par pedindo. E mede
    /// dos processos o que os dois juntos querem.
    pub(crate) fn ajustar_vigia(&mut self) {
        let precisa = self.assinantes > 0 || self.par_quer.is_some();
        if precisa != self.vigia.is_some() {
            if precisa {
                self.ligar_vigia();
            } else {
                self.vigia = None;
                tracing::info!("ninguém olhando: a coleta de uso parou");
            }
        }
        let mut interesse = self.dos_paineis[0].clone();
        if let Some((_, do_par)) = &self.par_quer {
            interesse = interesse.juntar(do_par);
        }
        if let Some(vigia) = &self.vigia
            && interesse != self.na_vigia
        {
            vigia.interessar(interesse.clone());
            self.na_vigia = interesse;
        }
    }

    fn ligar_vigia(&mut self) {
        let entregar = Arc::clone(&self.entregar);
        match Vigia::iniciar(move |amostra, processos| {
            entregar(Evento::Amostra(amostra, processos))
        }) {
            Ok(vigia) => {
                tracing::info!("um painel está olhando: a coleta de uso começou");
                self.vigia = Some(vigia);
                // A vigia nova não mede processo nenhum até ouvir o interesse.
                self.na_vigia = InteresseEmProcessos::default();
            }
            Err(erro) => tracing::warn!(%erro, "a coleta de uso não começou"),
        }
    }

    /// O quadro de agora, como o painel o veria.
    #[must_use]
    pub fn quadro(&self) -> ir_monitor::Quadro {
        self.mesa.quadro(Instant::now())
    }

    /// Os detalhes de agora: a cópia e os processos.
    #[must_use]
    pub fn detalhes(&self) -> ir_monitor::Detalhes {
        self.mesa.detalhes(Instant::now())
    }

    /// Se a coleta está ligada agora.
    #[must_use]
    pub const fn coletando(&self) -> bool {
        self.vigia.is_some()
    }
}

#[cfg(test)]
mod tests;
