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

use std::path::Path;

use anyhow::Result;
use ir_ipc::{Aviso, Falha, Resposta};
// A produção pergunta o portador em uso ao ator; só os testes nomeiam um portador à mão.
#[cfg(test)]
use ir_proto::carrier::Carrier;
use ir_proto::screens::Edge;
use ir_session::{Input, LinkDown, LocalIdentity, Phase, Role, Session, SessionConfig};
use tracing::{info, warn};

use super::Daemon;
use crate::config::Config;

/// Se esta máquina pode assumir `papel`, sabendo se a plataforma captura a entrada local.
///
/// Só o servidor depende de captura: é ele quem tem o teclado. Ser controlado funciona em toda
/// plataforma que injeta.
const fn papel_sustentado(papel: Role, captura: bool) -> bool {
    match papel {
        Role::Server => captura,
        Role::Client => true,
    }
}

/// O texto de configuração para um papel.
pub(super) const fn texto_do_papel(papel: Role) -> &'static str {
    match papel {
        Role::Server => "server",
        Role::Client => "client",
    }
}

/// O texto de configuração para uma borda.
const fn edge_para_texto(edge: Edge) -> &'static str {
    match edge {
        Edge::Left => "left",
        Edge::Right => "right",
        Edge::Top => "top",
        Edge::Bottom => "bottom",
    }
}

/// Uma sessão nova, desconectada, com este papel, esta borda e esta identidade.
///
/// O mesmo ponto para a subida do serviço e para a sessão recriada numa troca: duas maneiras de
/// montar a sessão acabariam montando duas sessões diferentes.
pub(crate) fn nova_sessao(papel: Role, edge: Edge, identidade: LocalIdentity) -> Session {
    let mut config = match papel {
        Role::Server => SessionConfig::server(edge),
        Role::Client => SessionConfig::client(edge),
    };
    // Uma semente nova a cada sessão criada — também na recriada por troca de papel.
    // Repetir a anterior faria o par tomar a sessão nova pela antiga (log 22).
    config.incarnation_seed = rand::random();
    Session::new(config, identidade)
}

/// O papel com que o serviço sobe, corrigindo um gravado que a plataforma não sustenta.
///
/// Sem isto, um servidor gravado num Linux subia num papel em que nada funciona, e nada dizia por
/// quê. Recusar-se a subir seria pior — o `systemd` o reiniciaria em laço. Então ele sobe como
/// cliente, registra em nível alto o que não aplicou, e corrige o arquivo para a janela e a
/// configuração dizerem a mesma coisa.
///
/// # Errors
///
/// Erro se o papel gravado não for texto reconhecido.
pub(crate) fn papel_na_subida(config: &mut Config, dir: &Path) -> Result<Role> {
    let gravado = config.session_role()?;
    if papel_sustentado(gravado, ir_input::capture_supported()) {
        return Ok(gravado);
    }
    warn!(
        gravado = texto_do_papel(gravado),
        "o papel gravado não funciona nesta plataforma, que não captura a entrada local; subindo \
         como cliente e corrigindo a configuração"
    );
    texto_do_papel(Role::Client).clone_into(&mut config.role);
    if let Err(erro) = config.save(dir) {
        warn!(%erro, "não foi possível corrigir o papel gravado; ele volta na próxima subida");
    }
    Ok(Role::Client)
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
        let mut nova = self.config.clone();
        texto.clone_into(&mut nova.role);
        let resposta = self.persistir(nova);
        if resposta == Resposta::Feito {
            info!(papel = texto, "papel trocado pela interface; já valendo");
            self.recriar_sessao(papel, self.edge);
        }
        resposta
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
        let mut nova = self.config.clone();
        texto.clone_into(&mut nova.peer_edge);
        if self.persistir(nova) == Resposta::Feito {
            info!(borda = texto, "borda anunciada pelo servidor; já valendo");
        } else {
            warn!(
                borda = texto,
                "borda anunciada pelo servidor vale nesta sessão, mas não foi gravada"
            );
        }
        let _ = self.avisos.send(Aviso::EstadoMudou(self.estado()));
    }

    /// Encerra a sessão em uso e põe no lugar uma nova, com este papel e esta borda.
    fn recriar_sessao(&mut self, papel: Role, edge: Edge) {
        // Primeiro soltar, depois qualquer outra coisa. `stop` se despede do par, emite o
        // `ReleaseAll` e devolve a entrada local — é o mesmo caminho de toda queda, e não uma
        // soltura escrita de novo aqui, onde poderia divergir.
        if self.session.phase() != Phase::Offline {
            let agora = self.now();
            self.session
                .stop(agora, LinkDown::UserStopped, &mut self.out);
            self.apply_commands();
        }

        self.session = nova_sessao(papel, edge, self.identidade_local.clone());
        self.edge = edge;
        if let Some(arranjo) = self.ultimo_arranjo.clone() {
            self.drive(Input::LocalScreens(arranjo));
        }
        self.garantir_entrada_local(papel);

        // Com o enlace seguro de pé, o aperto de mão recomeça já com o papel novo. O cursor real
        // e o modelo da sessão nova precisam começar no mesmo ponto.
        self.seed_pointer = true;
        self.last_phase = Phase::Offline;
        if self.linked {
            // Pelo portador que está de pé, e não por um presumido: trocar de papel sobre um
            // enlace de Bluetooth não pode reiniciar a sessão dizendo que ela é de rede.
            self.drive(Input::CarrierUp(self.portador_em_uso()));
        }
        let _ = self.avisos.send(Aviso::EstadoMudou(self.estado()));
    }

    /// No Linux o serviço injeta direto: um cliente precisa do injetor aberto.
    ///
    /// Quem subiu como servidor num Linux não abriu injetor nenhum, e voltar a cliente sem abrir
    /// um deixaria a máquina controlada sem ter como ser controlada.
    #[cfg(not(windows))]
    fn garantir_entrada_local(&mut self, papel: Role) {
        if papel != Role::Client || self.injector.is_some() {
            return;
        }
        match ir_input::open_injector() {
            Ok(injetor) => {
                info!("injetor aberto para o papel de cliente");
                self.injector = Some(injetor);
            }
            Err(erro) => warn!(%erro, "injeção local indisponível no papel de cliente"),
        }
    }

    /// No Windows quem captura e injeta é o agente, qualquer que seja o papel.
    #[cfg(windows)]
    #[allow(clippy::unused_self)]
    fn garantir_entrada_local(&mut self, _papel: Role) {}
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use ir_session::{Command, Notice};

    use super::*;
    use crate::actor::bancada::{Bancada, Diretorio, diretorio};

    fn daemon(papel: Role) -> (Daemon, Diretorio) {
        let bancada = Bancada::nova(papel);
        (bancada.daemon, bancada.dir)
    }

    fn gravado(dir: &Path) -> String {
        std::fs::read_to_string(dir.join("config.toml")).unwrap_or_default()
    }

    #[test]
    fn servidor_so_onde_ha_captura() {
        assert!(!papel_sustentado(Role::Server, false));
        assert!(papel_sustentado(Role::Server, true));
        assert!(papel_sustentado(Role::Client, false));
        assert!(papel_sustentado(Role::Client, true));
    }

    #[test]
    fn a_borda_nova_ja_vale_na_sessao_em_uso() {
        // O defeito era a janela mostrar a borda nova e a travessia usar a velha.
        let (mut daemon, dir) = daemon(Role::Server);
        assert_eq!(daemon.trocar_borda(Edge::Left), Resposta::Feito);
        assert_eq!(daemon.session.peer_edge(), Edge::Left);
        assert!(
            gravado(&dir).contains(r#"peer_edge = "left""#),
            "{}",
            gravado(&dir)
        );
    }

    #[test]
    fn trocar_a_borda_nao_refaz_a_sessao() {
        // Refazer mandava ao par um adeus de "encerrada pelo usuário", que ele não reconecta. Aqui
        // não há enlace: uma sessão refeita voltaria desligada, e a em uso continua no aperto de mão.
        let (mut daemon, _dir) = daemon(Role::Server);
        daemon.drive(Input::CarrierUp(Carrier::Udp));
        assert_eq!(daemon.session.phase(), Phase::Handshaking);

        assert_eq!(daemon.trocar_borda(Edge::Left), Resposta::Feito);

        assert_eq!(
            daemon.session.phase(),
            Phase::Handshaking,
            "a sessão é a mesma"
        );
        assert_eq!(daemon.session.peer_edge(), Edge::Left);
    }

    #[test]
    fn no_cliente_a_borda_e_do_servidor_e_nada_e_gravado() {
        let (mut daemon, dir) = daemon(Role::Client);
        assert_eq!(
            daemon.trocar_borda(Edge::Left),
            Resposta::Falha(Falha::BordaDoServidor)
        );
        assert_eq!(daemon.session.peer_edge(), Edge::Right, "a sessão não muda");
        assert!(
            !dir.join("config.toml").exists(),
            "uma recusa não pode gravar nada"
        );
    }

    #[test]
    fn o_cliente_grava_a_borda_que_o_servidor_anunciou() {
        // Sem gravar, o cliente subiria com a borda velha e atravessaria errado até reconectar.
        let (mut daemon, dir) = daemon(Role::Client);
        daemon
            .out
            .push(Command::Notify(Notice::EdgeChanged { edge: Edge::Left }));
        daemon.apply_commands();

        assert!(
            gravado(&dir).contains(r#"peer_edge = "left""#),
            "{}",
            gravado(&dir)
        );
        assert_eq!(daemon.estado().borda_do_par, ir_ipc::Borda::Esquerda);
    }

    #[test]
    fn o_papel_novo_ja_vale_na_sessao_em_uso() {
        // Voltar a cliente é sempre possível — é o caminho de quem ficou servidor por engano.
        let (mut daemon, dir) = daemon(Role::Server);
        assert_eq!(daemon.trocar_papel(Role::Client), Resposta::Feito);
        assert_eq!(daemon.session.role(), Role::Client);
        assert!(
            gravado(&dir).contains(r#"role = "client""#),
            "{}",
            gravado(&dir)
        );
    }

    #[test]
    fn servidor_sem_captura_e_recusado_sem_gravar_nada() {
        if ir_input::capture_supported() {
            return; // aqui o servidor é legítimo
        }
        let (mut daemon, dir) = daemon(Role::Client);
        assert_eq!(
            daemon.trocar_papel(Role::Server),
            Resposta::Falha(Falha::PapelIndisponivel)
        );
        assert_eq!(daemon.session.role(), Role::Client, "a sessão não muda");
        assert!(
            !dir.join("config.toml").exists(),
            "uma recusa não pode gravar nada"
        );
    }

    #[test]
    fn na_subida_um_servidor_sem_captura_vira_cliente_e_o_arquivo_e_corrigido() {
        let dir = diretorio();
        let mut config = Config {
            role: texto_do_papel(Role::Server).to_owned(),
            ..Config::default()
        };
        let papel = papel_na_subida(&mut config, &dir).expect("papel reconhecido");
        if ir_input::capture_supported() {
            assert_eq!(papel, Role::Server, "onde há captura, o gravado vale");
        } else {
            assert_eq!(papel, Role::Client);
            assert!(
                gravado(&dir).contains(r#"role = "client""#),
                "{}",
                gravado(&dir)
            );
        }
    }
}
