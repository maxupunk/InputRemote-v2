//! Quem pode controlar quem, e de que lado fica o outro — e fazer a mudança valer na hora.
//!
//! Não há papel ([ADR-0014](../../../../docs/adr/0014-controle-simetrico.md)): com a política
//! padrão, os dois computadores controlam um ao outro, e quem está usando agora é só a fase da
//! sessão. O que a pessoa escolhe é a política — os dois, só este, só o outro — e a borda.
//!
//! Mudar a política refaz a sessão: ela viaja no `Hello` (o par precisa saber se esta máquina aceita
//! ser controlada), e só um aperto de mão novo a leva. A sessão velha é encerrada pelo caminho que
//! solta tudo antes de qualquer outra coisa ([`Session::stop`]); a nova nasce com a mesma identidade,
//! o mesmo arranjo de telas e o ponteiro onde ele estava.
//!
//! A borda não refaz nada ([log 24](../../../../docs/logs/24-a-borda-e-do-servidor.md)): qualquer um
//! dos dois a muda na própria tela, a sessão em uso ajusta e avisa o par, e o par passa a usar a
//! oposta (`ir-session/src/session/edge.rs`).

use ir_ipc::{Aviso, Falha, Resposta};
// A produção pergunta o portador em uso ao ator; só os testes nomeiam um portador à mão.
#[cfg(test)]
use ir_proto::carrier::Carrier;
use ir_proto::screens::Edge;
use ir_session::{Input, LinkDown, LocalIdentity, Phase, Policy, Session, SessionConfig};
use tracing::{info, warn};

use super::Daemon;
use crate::config::{edge_para_texto, politica_sustentada, texto_da_politica};

/// Uma sessão nova, desconectada, com esta política, esta borda e esta identidade.
///
/// O mesmo ponto para a subida do serviço e para a sessão recriada numa mudança de política: duas
/// maneiras de montar a sessão acabariam montando duas sessões diferentes.
pub(crate) fn nova_sessao(
    politica: Policy,
    edge: Edge,
    identidade: LocalIdentity,
    borda_escolhida_em: Option<u64>,
) -> Session {
    let mut config = SessionConfig::new(edge);
    config.policy = politica;
    config.edge_chosen_at = borda_escolhida_em.unwrap_or(0);
    // Uma semente nova a cada sessão criada — também na recriada por mudança de política.
    // Repetir a anterior faria o par tomar a sessão nova pela antiga (log 22).
    config.incarnation_seed = rand::random();
    Session::new(config, identidade)
}

/// O relógio de parede, em milissegundos desde 1970, para comparar a escolha da borda entre as duas
/// máquinas. Um relógio antes de 1970 vira `0`, que perde para qualquer escolha.
fn agora_em_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
}

impl Daemon {
    /// Muda quem pode controlar quem, e a mudança já vale.
    pub(super) fn definir_politica(&mut self, politica: Policy) -> Resposta {
        let texto = texto_da_politica(politica);
        if !politica_sustentada(politica, ir_input::capture_supported()) {
            warn!(
                politica = texto,
                "política recusada: esta plataforma não lê o teclado daqui"
            );
            return Resposta::Falha(Falha::PoliticaIndisponivel);
        }
        if politica == self.session.policy() {
            return Resposta::Feito;
        }
        // Antes de gravar: aceitar e só depois descobrir que a captura não sobe deixava a máquina
        // sem ter o que mandar ao outro, e o ponteiro parado na borda (log 47).
        if politica == Policy::OnlyControls && !self.garantir_entrada_local().le {
            warn!(
                politica = texto,
                "política recusada: a captura local não abriu"
            );
            return Resposta::Falha(Falha::SemCaptura);
        }
        let resposta = self.persistir_com(|config| texto.clone_into(&mut config.politica));
        if resposta == Resposta::Feito {
            info!(
                politica = texto,
                "política mudada pela interface; já valendo"
            );
            self.recriar_sessao(politica);
        }
        resposta
    }

    /// Muda de que lado fica o outro computador, e a mudança já vale — sem derrubar a sessão.
    pub(super) fn trocar_borda(&mut self, edge: Edge) -> Resposta {
        if edge == self.edge() {
            return Resposta::Feito;
        }
        let texto = edge_para_texto(edge);
        let chosen_at = agora_em_ms();
        let resposta = self.persistir_com(|config| {
            texto.clone_into(&mut config.peer_edge);
            config.borda_escolhida_em = Some(chosen_at);
        });
        if resposta == Resposta::Feito {
            info!(borda = texto, "borda trocada pela interface; já valendo");
            // A sessão em uso ajusta a borda e avisa o par; se o controle estava atravessando,
            // volta antes. Nada de refazer a sessão.
            self.drive(Input::SetPeerEdge { edge, chosen_at });
            self.avisar_estado();
        }
        resposta
    }

    /// O outro computador mudou de lado na tela dele, e este passou a usar a borda oposta: grava,
    /// e a janela conta por que a posição mudou sozinha.
    pub(crate) fn adotar_borda(&mut self, edge: Edge, chosen_at: u64) {
        let texto = edge_para_texto(edge);
        // Sem esperar: o anúncio chega com a sessão de pé. Vale já; uma falha de gravação fica no
        // registro, e o par anuncia de novo na próxima sessão. O horário do par, e não o de agora:
        // senão esta ponta venceria a próxima comparação.
        self.gravar_ja(|config| {
            texto.clone_into(&mut config.peer_edge);
            config.borda_escolhida_em = Some(chosen_at);
        });
        info!(
            borda = texto,
            "o outro computador mudou de lado; este acompanhou"
        );
        let _ = self.avisos.send(Aviso::BordaAjustada(edge.into()));
        self.avisar_estado();
    }

    /// Encerra a sessão em uso e põe no lugar uma nova, com esta política.
    fn recriar_sessao(&mut self, politica: Policy) {
        // Primeiro soltar, depois qualquer outra coisa. Não "pedido pelo usuário": o par entenderia
        // pausa. A sessão nova vem em seguida.
        self.encerrar_sessao(LinkDown::Reconfiguring);
        // O ponteiro fica onde está: onde o serviço conduz o cursor (Linux), uma sessão nova no
        // canto faria o cursor saltar na próxima vez que a mão se mexesse.
        let (x, y) = self.session.pointer_xy();
        self.session = nova_sessao(
            politica,
            self.edge(),
            self.identidade_local.clone(),
            self.config.borda_escolhida_em,
        );
        // A sessão nova nasce sem saber nada: o mesmo que a da subida precisa saber.
        self.alimentar_sessao_nova();
        self.session.sync_pointer(x, y);
        let _ = self.garantir_entrada_local();
        self.last_phase = Phase::Offline;
        // Pelos portadores que estão de pé, e não por um presumido: refazer a sessão sobre um
        // enlace de Bluetooth não pode reiniciá-la dizendo que ela é de rede.
        self.retomar_sessao();
        self.avisar_estado();
    }
}

#[cfg(test)]
mod tests;
