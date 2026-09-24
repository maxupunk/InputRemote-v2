//! Trocar o papel e a borda desta máquina — e fazer a troca valer na hora.
//!
//! Antes, as duas trocas só gravavam a configuração, e a sessão em uso continuava com o valor
//! antigo até o serviço reiniciar. No papel, isso fez um Linux virar servidor em silêncio, quase
//! três horas depois do clique ([log 18](../../../../docs/logs/18-a-prova-no-notebook-e-a-troca-de-papel.md)).
//! Na borda era pior: a janela passava a mostrar a borda nova, e a travessia continuava usando a
//! velha.
//!
//! Agora a troca de papel refaz a sessão com o valor novo. A sessão velha é encerrada pelo caminho
//! que solta tudo antes de qualquer outra coisa ([`Session::stop`]), a nova nasce com a mesma
//! identidade e o mesmo arranjo de telas, e, se havia enlace, o aperto de mão recomeça com o papel
//! certo. E um papel que a plataforma não sustenta é recusado com o motivo, em vez de gravado.
//!
//! A borda é diferente, e não refaz nada ([log 24](../../../../docs/logs/24-a-borda-e-do-servidor.md)).
//! Refazer a sessão a cada clique mandava ao par um adeus que ainda dizia para não reconectar. E a
//! borda é do servidor: só ele escolhe, a sessão em uso ajusta a borda e avisa o cliente, e o
//! cliente grava a oposta.

use ir_ipc::{Aviso, Falha, Resposta};
// A produção pergunta o portador em uso ao ator; só os testes nomeiam um portador à mão.
#[cfg(test)]
use ir_proto::carrier::Carrier;
use ir_proto::screens::Edge;
use ir_session::{Input, LinkDown, LocalIdentity, Phase, Role, Session, SessionConfig};
use tracing::{info, warn};

use super::Daemon;
use crate::config::{edge_para_texto, papel_sustentado, texto_do_papel};

/// Uma sessão nova, desconectada, com este papel, esta borda e esta identidade.
///
/// O mesmo ponto para a subida do serviço e para a sessão recriada numa troca: duas maneiras de
/// montar a sessão acabariam montando duas sessões diferentes.
pub(crate) fn nova_sessao(
    papel: Role,
    edge: Edge,
    identidade: LocalIdentity,
    escolhido_em: Option<u64>,
) -> Session {
    let mut config = match papel {
        Role::Server => SessionConfig::server(edge),
        Role::Client => SessionConfig::client(edge),
    };
    // Uma semente nova a cada sessão criada — também na recriada por troca de papel.
    // Repetir a anterior faria o par tomar a sessão nova pela antiga (log 22).
    config.incarnation_seed = rand::random();
    config.role_chosen_at = escolhido_em.unwrap_or(0);
    Session::new(config, identidade)
}

/// O relógio de parede, em milissegundos desde 1970, para comparar escolhas de papel entre as duas
/// máquinas. Um relógio antes de 1970 vira `0`, que perde para qualquer escolha.
fn agora_em_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
}

impl Daemon {
    /// Troca o papel desta máquina, e a troca já vale.
    pub(super) fn trocar_papel(&mut self, papel: Role) -> Resposta {
        let texto = texto_do_papel(papel);
        if !papel_sustentado(papel, ir_input::capture_supported()) {
            warn!(
                papel = texto,
                "troca de papel recusada: esta plataforma não captura a entrada local, então não \
                 pode ser a que tem o teclado"
            );
            return Resposta::Falha(Falha::PapelIndisponivel);
        }
        if papel == self.session.role() {
            return Resposta::Feito;
        }
        // Antes de gravar: aceitar e só depois descobrir que a captura não sobe deixava a máquina
        // com o teclado sem ter o que capturar, e o ponteiro parado na borda (log 47).
        if papel == Role::Server && !self.garantir_entrada_local(papel) {
            warn!(
                papel = texto,
                "troca de papel recusada: a captura local não abriu"
            );
            return Resposta::Falha(Falha::SemCaptura);
        }
        let mut nova = self.config.clone();
        texto.clone_into(&mut nova.role);
        nova.papel_escolhido_em = Some(agora_em_ms());
        let resposta = self.persistir(nova);
        if resposta == Resposta::Feito {
            info!(papel = texto, "papel trocado pela interface; já valendo");
            self.recriar_sessao(papel, self.edge);
        }
        resposta
    }

    /// O outro computador escolheu o mesmo papel, depois: este passa ao complementar.
    ///
    /// É o que deixa trocar o papel numa tela só — a outra se ajusta sozinha. Grava o horário da
    /// escolha do par, e não o de agora: gravar o de agora faria esta ponta vencer a próxima
    /// comparação e ceder de volta.
    pub(crate) fn adotar_papel(&mut self, papel: Role, escolhido_em: u64) {
        if papel == self.session.role() {
            return;
        }
        let texto = texto_do_papel(papel);
        if !papel_sustentado(papel, ir_input::capture_supported()) {
            warn!(
                papel = texto,
                "o outro computador tem o mesmo papel, mas esta plataforma não sustenta o outro"
            );
            return;
        }
        if papel == Role::Server && !self.garantir_entrada_local(papel) {
            // Os dois ficam com o mesmo papel; a tela daqui diz por quê e o que fazer.
            warn!(
                papel = texto,
                "o outro computador trocou de papel, mas a entrada local não abriu"
            );
            let _ = self.avisos.send(Aviso::Falhou(Falha::SemCaptura));
            return;
        }
        let mut nova = self.config.clone();
        texto.clone_into(&mut nova.role);
        nova.papel_escolhido_em = Some(escolhido_em);
        if self.persistir(nova) == Resposta::Feito {
            info!(
                papel = texto,
                "o outro computador trocou de papel; este se ajustou"
            );
            self.recriar_sessao(papel, self.edge);
            let _ = self
                .avisos
                .send(Aviso::PapelAjustado(ir_painel::papel_de(papel)));
        }
    }

    /// Troca a borda de travessia, e a troca já vale — sem derrubar a sessão.
    ///
    /// Só no servidor. A borda é de quem tem o teclado e o mouse: o cliente usa a oposta da que o
    /// servidor anunciar, e deixar os dois escolherem foi o que deixou a bancada com `left` dos dois
    /// lados.
    pub(super) fn trocar_borda(&mut self, edge: Edge) -> Resposta {
        if self.session.role() != Role::Server {
            return Resposta::Falha(Falha::BordaDoServidor);
        }
        if edge == self.edge {
            return Resposta::Feito;
        }
        let texto = edge_para_texto(edge);
        let mut nova = self.config.clone();
        texto.clone_into(&mut nova.peer_edge);
        let resposta = self.persistir(nova);
        if resposta == Resposta::Feito {
            info!(borda = texto, "borda trocada pela interface; já valendo");
            self.edge = edge;
            // A sessão em uso ajusta a borda e avisa o cliente; se o controle estava com ele, volta
            // antes. Nada de refazer a sessão.
            self.drive(Input::SetPeerEdge(edge));
            let _ = self.avisos.send(Aviso::EstadoMudou(self.estado()));
        }
        resposta
    }

    /// A sessão passou a usar esta borda: grava, e conta às interfaces.
    ///
    /// É o caminho do cliente, que recebe a borda do servidor. No servidor a troca já foi gravada
    /// por [`Self::trocar_borda`] antes de chegar à sessão, e aqui não sobra nada a fazer.
    pub(crate) fn adotar_borda(&mut self, edge: Edge) {
        if edge == self.edge {
            return;
        }
        self.edge = edge;
        let texto = edge_para_texto(edge);
        // Sem esperar: o anúncio chega com a sessão de pé. Vale já; uma falha de gravação fica no
        // registro, e o servidor anuncia de novo na próxima sessão.
        texto.clone_into(&mut self.config.peer_edge);
        self.gravador.gravar(&self.config);
        info!(borda = texto, "borda anunciada pelo servidor; já valendo");
        let _ = self.avisos.send(Aviso::EstadoMudou(self.estado()));
    }

    /// Encerra a sessão em uso e põe no lugar uma nova, com este papel e esta borda.
    fn recriar_sessao(&mut self, papel: Role, edge: Edge) {
        // Primeiro soltar, depois qualquer outra coisa. `stop` se despede do par, emite o
        // `ReleaseAll` e devolve a entrada local — é o mesmo caminho de toda queda, e não uma
        // soltura escrita de novo aqui, onde poderia divergir.
        if self.session.phase() != Phase::Offline {
            let agora = self.now();
            // Não "pedido pelo usuário": o par entenderia pausa. A sessão nova vem em seguida.
            self.session
                .stop(agora, LinkDown::Reconfiguring, &mut self.out);
            self.apply_commands();
        }

        self.session = nova_sessao(
            papel,
            edge,
            self.identidade_local.clone(),
            self.config.papel_escolhido_em,
        );
        self.edge = edge;
        if let Some(arranjo) = self.ultimo_arranjo.clone() {
            self.drive(Input::LocalScreens(arranjo));
        }
        let _ = self.garantir_entrada_local(papel);

        // Com o enlace seguro de pé, o aperto de mão recomeça já com o papel novo. O cursor real
        // e o modelo da sessão nova precisam começar no mesmo ponto.
        self.seed_pointer = true;
        self.last_phase = Phase::Offline;
        // A sessão nova nasce sem a fixação de portador e sem o rádio daqui.
        let fixado = self.portador_fixado.map(ir_ipc::Portador::no_protocolo);
        self.session.pin_carrier(fixado, &mut self.out);
        self.apply_commands();
        if self.borda_travada {
            self.drive(ir_session::Input::LockEdge(true));
        }
        self.anunciar_radio_proprio();
        // Pelos portadores que estão de pé, e não por um presumido: trocar de papel sobre um
        // enlace de Bluetooth não pode reiniciar a sessão dizendo que ela é de rede.
        self.retomar_sessao();
        let _ = self.avisos.send(Aviso::EstadoMudou(self.estado()));
    }

    /// No Linux o serviço injeta direto: um cliente precisa do injetor aberto.
    ///
    /// Quem subiu como servidor num Linux não abriu injetor nenhum, e voltar a cliente sem abrir
    /// um deixaria a máquina controlada sem ter como ser controlada.
    ///
    /// Devolve se a entrada que o papel precisa está aberta.
    #[cfg(not(windows))]
    pub(crate) fn garantir_entrada_local(&mut self, papel: Role) -> bool {
        let pronta = self.abrir_entrada_local(papel);
        self.ajustar_conducao();
        pronta
    }

    /// Abre o que o papel precisa. O injetor serve aos dois: o cliente digita o que vem do par, e o
    /// servidor conduz o cursor daqui com ele ([`super::cursor`]).
    #[cfg(not(windows))]
    fn abrir_entrada_local(&mut self, papel: Role) -> bool {
        if papel == Role::Server && self.capturer.is_none() {
            match crate::fundo::capturar(&self.captura) {
                Ok(capturador) => {
                    info!("captura ligada para o papel de servidor");
                    self.capturer = Some(capturador);
                }
                Err(erro) => warn!(%erro, "captura local indisponível no papel de servidor"),
            }
        }
        if self.injector.is_none() {
            match ir_input::open_injector() {
                Ok(injetor) => {
                    info!("injetor aberto");
                    self.injector = Some(injetor);
                }
                Err(erro) => warn!(%erro, "injeção local indisponível"),
            }
        }
        match papel {
            Role::Server => self.capturer.is_some(),
            Role::Client => self.injector.is_some(),
        }
    }

    /// Com o teclado aqui e a captura parada, tenta de novo — em silêncio até conseguir.
    ///
    /// A captura que falhou na subida (o teclado USB que ainda não tinha aparecido, uma permissão
    /// dada depois) ficava parada até reiniciar o serviço. Agora a máquina se recupera sozinha, e a
    /// tela sai do aviso quando a captura sobe (log 47).
    #[cfg(not(windows))]
    pub(super) fn recuperar_captura(&mut self) {
        if self.session.role() != Role::Server || self.capturer.is_some() {
            return;
        }
        match crate::fundo::capturar(&self.captura) {
            Ok(capturador) => {
                info!("captura ligada: este computador voltou a poder ter o teclado");
                self.capturer = Some(capturador);
                self.ajustar_conducao();
                let _ = self.avisos.send(Aviso::EstadoMudou(self.estado()));
            }
            Err(erro) => tracing::debug!(%erro, "a captura ainda não abre"),
        }
    }

    /// No Windows quem captura é o agente, e quem o traz de volta é o laço do agente.
    #[cfg(windows)]
    #[allow(clippy::unused_self)]
    pub(super) const fn recuperar_captura(&mut self) {}

    /// No Windows quem captura e injeta é o agente, qualquer que seja o papel.
    #[cfg(windows)]
    #[allow(clippy::unused_self)]
    pub(crate) const fn garantir_entrada_local(&mut self, _papel: Role) -> bool {
        true
    }
}

#[cfg(test)]
mod tests;
