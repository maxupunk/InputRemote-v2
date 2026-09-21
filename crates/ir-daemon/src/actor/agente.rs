//! A face do ator para o agente: os fatos que chegam dele, e a garantia de que ele existe.
//!
//! O agente é **descartável** ([02, §1.2](../../../docs/02-arquitetura.md)): ele não guarda
//! estado, e morrer não custa nada além de um relançamento. Por isso o que este módulo faz é
//! simples de propósito — traduzir fato em entrada da sessão, e ressubir o agente quando ele
//! não estiver de pé.

use ir_ipc::{Aviso, ComandoDoAgente, FatoDoAgente};
use ir_proto::input::PointerDelta;
use ir_session::Input;
use tokio::sync::broadcast;
use tracing::{info, warn};

use super::Daemon;

/// A cada quantas batidas de 5 ms se relança o agente que ainda não conectou.
///
/// **Precisa ser maior que a janela em que o agente desiste**, que é de 30 s — 60 tentativas de
/// 500 ms, em `ir-agent`. Eram 9 s, e daí vinham dois efeitos que se escondiam um no outro: o
/// agente nunca chegava ao fim das próprias tentativas, então o erro que diz *por que* ele não
/// conecta jamais aparecia; e cada lançamento sobrevivia ao seguinte, de modo que os processos se
/// empilhavam — exatamente o que o espaçamento existia para evitar (log 29).
///
/// Os dois números vivem em crates diferentes e não há como o compilador amarrá-los. Se um mudar,
/// o outro precisa ser conferido à mão: 36 s aqui contra 30 s lá.
///
/// Só existe no Windows: no Linux não há agente a relançar, e uma constante sem uso lá viraria
/// aviso de build.
#[cfg(windows)]
const RELANCAR_AGENTE_TICKS: u32 = super::RECONNECT_TICKS * 12;

impl Daemon {
    /// Por onde mandar comandos ao agente — só quando há agente pronto.
    ///
    /// É isto que faz o serviço usar o agente no Windows e o `uinput` direto no Linux, sem um
    /// `if` de plataforma espalhado pelo despacho: no Linux nenhum agente conecta, então a
    /// resposta é sempre `None` e o caminho local vale.
    pub(crate) fn comandos_do_agente(&self) -> Option<&broadcast::Sender<ComandoDoAgente>> {
        if self.agente_pronto {
            Some(&self.agente)
        } else {
            None
        }
    }

    /// Um fato vindo do agente, traduzido para entrada da sessão.
    pub(super) fn on_fato(&mut self, fato: FatoDoAgente) {
        let input = match fato {
            FatoDoAgente::Pronto { desktops } => {
                self.on_agente_pronto(&desktops);
                return;
            }
            FatoDoAgente::Encerrou => {
                self.on_agente_encerrou();
                return;
            }
            // A posição absoluta não é movimento: ela **semeia** ou sincroniza o ponteiro, que é
            // o que faz a travessia disparar na borda certa.
            FatoDoAgente::PonteiroAbsoluto { x, y } => {
                self.on_absolute_pointer(x, y);
                return;
            }
            FatoDoAgente::PonteiroLocal { dx, dy } => Input::LocalPointer(PointerDelta { dx, dy }),
            FatoDoAgente::RodaLocal(delta) => Input::LocalWheel(delta),
            FatoDoAgente::TeclaLocal { usage, pressionada } => Input::LocalKey {
                usage,
                pressed: pressionada,
            },
            FatoDoAgente::BotaoLocal { botao, pressionado } => Input::LocalButton {
                button: botao,
                pressed: pressionado,
            },
            // Quem sabe o tamanho da tela do usuário é quem está na sessão dele.
            FatoDoAgente::TelasMudaram(arranjo) => {
                self.definir_telas(arranjo);
                return;
            }
            FatoDoAgente::InjecaoRecusada { desktop } => {
                warn!(desktop, "o sistema recusou a injeção do agente");
                return;
            }
            FatoDoAgente::DesktopMudou { nome } => {
                info!(nome, "o desktop de entrada mudou");
                return;
            }
            _ => return,
        };
        self.drive(input);
    }

    /// O agente conectou e está pronto para capturar e injetar.
    fn on_agente_pronto(&mut self, desktops: &[String]) {
        self.agente_pronto = true;
        info!(?desktops, "agente pronto");
        // O cursor real está onde está, e o modelo da sessão precisa recomeçar no mesmo ponto.
        self.seed_pointer = true;
        let _ = self.avisos.send(Aviso::EstadoMudou(self.estado()));
    }

    /// O agente saiu, ou a conexão com ele caiu.
    ///
    /// Não há o que soltar por aqui: quem segurava teclas era o agente, e a supressão da entrada
    /// local morreu junto com os ganchos dele — o teclado do usuário volta sozinho. O que importa
    /// é parar de contar com ele, e dizer isso à interface em vez de fingir que está tudo bem.
    fn on_agente_encerrou(&mut self) {
        if !self.agente_pronto {
            return;
        }
        self.agente_pronto = false;
        warn!("o agente saiu; sem captura nem injeção até ele voltar");
        let _ = self.avisos.send(Aviso::EstadoMudou(self.estado()));
    }

    /// Garante que há um agente de pé, relançando-o quando não há.
    ///
    /// Chamado uma vez na subida — e aí o agente nasce junto com o serviço — e depois no
    /// intervalo da reconexão de rede. Relançar um agente que morreu é barato, e é o que faz a
    /// falha dele ser um soluço em vez de o fim da sessão.
    #[cfg(windows)]
    pub(crate) fn garantir_agente(&mut self) {
        if self.agente_pronto {
            return;
        }
        if !self.ticks.is_multiple_of(RELANCAR_AGENTE_TICKS) {
            return;
        }
        match ir_sessao::lancar_agente() {
            Ok(pid) => info!(pid, "agente lançado"),
            Err(erro) => warn!(%erro, "não foi possível lançar o agente"),
        }
    }

    /// No Linux o serviço injeta direto por `uinput`, e não há agente a lançar
    /// ([06, §2](../../../docs/06-linux.md)).
    #[cfg(not(windows))]
    #[allow(clippy::unused_self)]
    pub(crate) fn garantir_agente(&mut self) {}
}
