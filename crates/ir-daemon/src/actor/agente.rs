//! A face do ator para o agente: os fatos que chegam dele, e a garantia de que ele existe.
//!
//! O agente é **descartável** ([02, §1.2](../../../docs/02-arquitetura.md)): ele não guarda
//! estado, e morrer não custa nada além de um relançamento. Por isso o que este módulo faz é
//! simples de propósito — traduzir fato em entrada da sessão, e ressubir o agente quando ele
//! não estiver de pé.

use std::time::{Duration, Instant};

use ir_ipc::{ComandoDoAgente, FatoDoAgente};
use ir_session::Input;
use tokio::sync::broadcast;
use tracing::{info, warn};

use super::Daemon;

/// Quanto se espera o agente lançado conectar antes de lançar outro.
///
/// **Precisa ser maior que a janela em que o agente desiste**, que é de 30 s — 60 tentativas de
/// 500 ms, em `ir-agent`. Eram 9 s, e daí vinham dois efeitos que se escondiam um no outro: o
/// agente nunca chegava ao fim das próprias tentativas, então o erro que diz *por que* ele não
/// conecta jamais aparecia; e cada lançamento sobrevivia ao seguinte, de modo que os processos se
/// empilhavam — exatamente o que o espaçamento existia para evitar (log 29).
///
/// Os dois números vivem em crates diferentes e não há como o compilador amarrá-los. Se um mudar,
/// o outro precisa ser conferido à mão: 36 s aqui contra 30 s lá.
const RELANCAR_AGENTE: Duration = Duration::from_secs(36);

/// Os desktops em que o agente injeta, como ele os contou ao ficar pronto.
#[derive(Debug, Default)]
pub(crate) struct DesktopsDoAgente {
    /// Os nomes, para o diagnóstico.
    pub(crate) nomes: Vec<String>,
    /// Se entre eles está o seguro — a tela de bloqueio. Quem decide pelo nome é o agente.
    pub(crate) tela_de_bloqueio: bool,
}

/// O zelador do agente: o mesmo do ajudante de clipboard, com o prazo do agente.
pub(super) const fn zelador() -> ir_sessao::Zelador {
    ir_sessao::Zelador::novo("agente", RELANCAR_AGENTE)
}

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

    /// Um fato vindo do agente.
    ///
    /// A entrada que o agente captura é do mesmo tipo que a captura local do Linux entrega, e segue
    /// pelo mesmo caminho ([`Self::on_capture`]), sem tradução.
    pub(super) fn on_fato(&mut self, fato: FatoDoAgente) {
        match fato {
            FatoDoAgente::Pronto {
                desktops,
                tela_de_bloqueio,
            } => self.on_agente_pronto(&desktops, tela_de_bloqueio),
            FatoDoAgente::Encerrou => self.on_agente_encerrou(),
            FatoDoAgente::Capturado(evento) => self.on_capture(evento),
            // Quem sabe o tamanho da tela do usuário é quem está na sessão dele.
            FatoDoAgente::TelasMudaram(arranjo) => self.definir_telas(arranjo),
            // Se o desktop é protegido, o agente já disse: a regra é uma só, a de `ir_input::desktop`.
            FatoDoAgente::InjecaoRecusada { desktop, protegido } => {
                if protegido {
                    warn!(desktop, "a injeção do agente foi recusada");
                    self.recusando_protegido(true);
                } else {
                    self.injecao_recusada_na_area_de_trabalho();
                }
            }
            FatoDoAgente::DesktopMudou { nome, protegido } => {
                self.on_desktop_mudou(&nome, protegido);
            }
            _ => {}
        }
    }

    /// O sistema recusou uma injeção na área de trabalho: a tela passa a dizer, e o registro anota
    /// só a mudança — uma recusa por evento inundava o registro sem explicar nada a ninguém.
    fn injecao_recusada_na_area_de_trabalho(&mut self) {
        let nova = self.injecao_recusada.is_none();
        self.injecao_recusada = Some(std::time::Instant::now());
        if nova {
            warn!("o sistema está recusando o teclado e o mouse que o par manda");
            self.avisar_estado();
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
            self.avisar_estado();
        }
    }

    /// O agente conectou e está pronto para capturar e injetar.
    fn on_agente_pronto(&mut self, desktops: &[String], tela_de_bloqueio: bool) {
        self.agente_pronto = true;
        self.desktops_do_agente = DesktopsDoAgente {
            nomes: desktops.to_vec(),
            tela_de_bloqueio,
        };
        // O par fica sabendo na próxima sessão: é no `Hello` que as capacidades viajam.
        self.identidade_local.capabilities.privileged_input = self.nivel_daqui().no_protocolo();
        self.drive(Input::AgentReady);
        info!(?desktops, "agente pronto");
        self.contar_ao_agente_a_permissao();
        // O cursor real está onde está, e o modelo da sessão precisa recomeçar no mesmo ponto.
        self.seed_pointer = true;
        self.avisar_estado();
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
        self.avisar_estado();
        self.garantir_agente_agora();
    }

    /// Lança o agente já, se ele não está pronto, sem esperar a rodada de reconexão
    /// ([02, §8](../../../docs/02-arquitetura.md) promete meio segundo): na subida, quando ele sai,
    /// quando a sessão do Windows muda e quando a máquina acorda. Se ele não conectar, a rodada
    /// periódica ([`Self::garantir_agente`]) volta a tentar.
    ///
    /// No Linux o serviço injeta direto por `uinput`, e não há agente a lançar
    /// ([06, §2](../../../docs/06-linux.md)); nos testes não se lança processo nenhum.
    #[cfg_attr(
        any(not(windows), test),
        allow(clippy::unused_self, clippy::missing_const_for_fn)
    )]
    pub(crate) fn garantir_agente_agora(&mut self) {
        #[cfg(all(windows, not(test)))]
        if !self.agente_pronto {
            self.zelador_do_agente
                .lancar(ir_sessao::lancar_agente, Instant::now());
        }
    }

    /// O desktop de entrada desta máquina mudou: bloqueou, abriu o UAC, voltou à área de trabalho.
    ///
    /// Com o controle no par, a tela que bloqueia leva o controle de volta: os ganchos não veem o
    /// Win+L nem o desktop seguro, e o par ficaria recebendo o que ninguém mais digita
    /// ([05, §5.1](../../../docs/05-windows.md)). Do lado controlado, voltar à área de trabalho
    /// encerra a recusa do desktop protegido.
    fn on_desktop_mudou(&mut self, nome: &str, protegido: bool) {
        info!(nome, "o desktop de entrada mudou");
        self.on_tela_protegida(protegido);
        if protegido && self.session.phase() == ir_session::Phase::Sending {
            info!("a tela daqui bloqueou com o controle no par: devolvendo e soltando tudo");
            self.drive(Input::EmergencyRelease);
        }
    }

    /// Conta ao agente se o par pode digitar na tela de bloqueio e no UAC.
    pub(crate) fn contar_ao_agente_a_permissao(&self) {
        let permitido = self.protegido_permitido();
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

    /// Garante, no intervalo da reconexão de rede, que há um agente de pé: sem agente pronto, relança
    /// [`RELANCAR_AGENTE`] depois do último lançamento. Relançar um agente que morreu é barato, e é o
    /// que faz a falha dele ser um soluço em vez de o fim da sessão.
    pub(crate) fn garantir_agente(&mut self) {
        if self
            .zelador_do_agente
            .conferir(self.agente_pronto, Instant::now())
        {
            self.garantir_agente_agora();
        }
    }
}

#[cfg(test)]
mod testes;
