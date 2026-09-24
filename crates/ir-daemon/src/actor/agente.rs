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
                if desktop.eq_ignore_ascii_case("Default") {
                    self.injecao_recusada_na_area_de_trabalho();
                } else {
                    warn!(desktop, "a injeção do agente foi recusada");
                    self.recusando_protegido(true);
                }
                return;
            }
            FatoDoAgente::DesktopMudou { nome } => {
                self.on_desktop_mudou(&nome);
                return;
            }
            _ => return,
        };
        self.drive(input);
    }

    /// O sistema recusou uma injeção na área de trabalho: a tela passa a dizer, e o registro anota
    /// só a mudança — uma recusa por evento inundava o registro sem explicar nada a ninguém.
    fn injecao_recusada_na_area_de_trabalho(&mut self) {
        let nova = self.injecao_recusada.is_none();
        self.injecao_recusada = Some(std::time::Instant::now());
        if nova {
            warn!("o sistema está recusando o teclado e o mouse que o par manda");
            let _ = self.avisos.send(ir_ipc::Aviso::EstadoMudou(self.estado()));
        }
    }

    /// Sem recusa há alguns segundos, a injeção voltou a passar: a tela sai do aviso.
    pub(super) fn esquecer_recusa_antiga(&mut self) {
        let antiga = self
            .injecao_recusada
            .is_some_and(|quando| quando.elapsed() > std::time::Duration::from_secs(5));
        if antiga {
            info!("o sistema voltou a aceitar o teclado e o mouse do par");
            self.injecao_recusada = None;
            let _ = self.avisos.send(ir_ipc::Aviso::EstadoMudou(self.estado()));
        }
    }

    /// O agente conectou e está pronto para capturar e injetar.
    fn on_agente_pronto(&mut self, desktops: &[String]) {
        self.agente_pronto = true;
        self.desktops_do_agente = desktops.to_vec();
        // O par fica sabendo na próxima sessão: é no `Hello` que as capacidades viajam.
        self.identidade_local.capabilities.privileged_input = match self.nivel_daqui() {
            ir_ipc::Nivel::TelaDeLogin => ir_proto::peer::PrivilegedInputLevel::LoginScreen,
            ir_ipc::Nivel::TelaDeBloqueio => ir_proto::peer::PrivilegedInputLevel::LockScreen,
            ir_ipc::Nivel::SoDesbloqueado => ir_proto::peer::PrivilegedInputLevel::UnlockedOnly,
            ir_ipc::Nivel::Nenhum => ir_proto::peer::PrivilegedInputLevel::None,
        };
        self.drive(Input::AgentReady);
        info!(?desktops, "agente pronto");
        self.contar_ao_agente_a_permissao();
        // O cursor real está onde está, e o modelo da sessão precisa recomeçar no mesmo ponto.
        self.seed_pointer = true;
        let _ = self.avisos.send(Aviso::EstadoMudou(self.estado()));
    }

    /// O agente saiu, ou a conexão com ele caiu.
    ///
    /// O agente solta o que ele injetou ao sair, e a supressão da entrada local morre junto com os
    /// ganchos dele. O que falta é o lado da **sessão**: se esta máquina controlava o par, as
    /// teclas que desceram lá nunca vão ter a subida; se era controlada, o controle volta a quem
    /// digita. É o `AgentLost` que faz as duas coisas. Depois, o agente é relançado na hora, sem
    /// esperar a próxima rodada de reconexão.
    fn on_agente_encerrou(&mut self) {
        if !self.agente_pronto {
            return;
        }
        self.agente_pronto = false;
        warn!("o agente saiu; sem captura nem injeção até ele voltar");
        self.drive(Input::AgentLost);
        let _ = self.avisos.send(Aviso::EstadoMudou(self.estado()));
        self.relancar_agente_ja();
    }

    /// Relança o agente sem esperar a rodada de reconexão ([02, §8](../../../docs/02-arquitetura.md)
    /// promete meio segundo). Se ele não conectar, a rodada periódica volta a tentar.
    /// Nos testes não se lança processo nenhum.
    #[cfg(all(windows, not(test)))]
    #[allow(clippy::unused_self)]
    pub(super) fn relancar_agente_ja(&mut self) {
        match ir_sessao::lancar_agente() {
            Ok(pid) => info!(pid, "agente relançado"),
            Err(erro) => warn!(%erro, "não foi possível relançar o agente"),
        }
    }

    #[cfg(any(not(windows), test))]
    #[allow(clippy::unused_self)]
    pub(super) fn relancar_agente_ja(&mut self) {}

    /// O desktop de entrada desta máquina mudou: bloqueou, abriu o UAC, voltou à área de trabalho.
    ///
    /// Do lado que controla, a tela que bloqueia leva o controle de volta: os ganchos não veem o
    /// Win+L nem o desktop seguro, e o par ficaria recebendo o que ninguém mais digita
    /// ([05, §5.1](../../../docs/05-windows.md)). Do lado controlado, voltar à área de trabalho
    /// encerra a recusa do desktop protegido.
    fn on_desktop_mudou(&mut self, nome: &str) {
        info!(nome, "o desktop de entrada mudou");
        if nome.eq_ignore_ascii_case("Default") {
            self.recusando_protegido(false);
        } else if self.session.role() == ir_session::Role::Server
            && self.session.phase() == ir_session::Phase::Engaged
        {
            info!("a tela daqui bloqueou com o controle no par: devolvendo e soltando tudo");
            self.drive(Input::EmergencyRelease);
        }
    }

    /// Conta ao agente se o par pode digitar na tela de bloqueio e no UAC.
    pub(crate) fn contar_ao_agente_a_permissao(&self) {
        let permitido = self
            .config
            .peers
            .first()
            .is_some_and(|par| par.tela_de_bloqueio);
        if let Some(agente) = self.comandos_do_agente() {
            let _ = agente.send(ComandoDoAgente::PermitirDesktopProtegido(permitido));
        }
    }

    /// Renova a supressão da entrada local no agente enquanto ela vale.
    ///
    /// O agente devolve o teclado e o mouse ao usuário se a renovação parar de chegar: é o que
    /// impede um serviço travado de deixar a máquina sem entrada (`ir-agent`, `vigia`).
    pub(crate) fn renovar_supressao(&self) {
        if self.suprimindo
            && let Some(agente) = self.comandos_do_agente()
        {
            let _ = agente.send(ComandoDoAgente::SuprimirEntradaLocal(true));
        }
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

#[cfg(test)]
mod testes;
