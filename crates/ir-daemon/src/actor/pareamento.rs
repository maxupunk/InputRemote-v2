//! O prazo do pareamento: o código vale dois minutos, e depois disso o serviço desiste.
//!
//! Sem prazo, um código que ninguém confirmou deixava o serviço esperando para sempre — e, como
//! quem espera confirmação não tenta reconectar, a máquina ficava fora do ar até alguém reiniciar
//! o serviço à mão. A interface já prometia "o código vale 2 minutos"; aqui a promessa passa a ser
//! verdade.
//!
//! O mesmo vale para o enlace que cai com o código na tela: não há mais o que confirmar, e ficar
//! esperando é o mesmo defeito por outro caminho.

use std::time::{Duration, Instant};

use ir_ipc::{Aviso, Resposta};
use ir_session::{LinkDown, Phase};
use tracing::{info, warn};

use super::Daemon;

/// Quanto tempo o código de pareamento vale.
///
/// O mesmo número que a interface mostra ao usuário em [`ir_ipc::Falha::PareamentoExpirou`]:
/// dois números diferentes para a mesma coisa seriam uma promessa quebrada.
const PRAZO_DO_PAREAMENTO: Duration = Duration::from_secs(120);

/// Um pareamento em andamento: do código na tela até o fim.
///
/// Termina com o par gravado, com o enlace caindo, com a recusa ou no prazo — e não no clique em
/// "São iguais", que só diz que deste lado confere (log 25).
#[derive(Debug, Clone, Copy)]
pub(crate) struct Pareamento {
    /// Quando o código apareceu. O prazo conta daqui.
    pub(super) desde: Instant,
    /// Se o usuário já disse que o código confere, e só falta o outro computador.
    pub(super) conferido: bool,
}

/// Se um pareamento começado em `desde` já passou do prazo em `agora`.
///
/// Relógio monotônico, e saturando: um `agora` anterior a `desde` conta como tempo zero, e nunca
/// como código vencido.
fn expirou(desde: Instant, agora: Instant) -> bool {
    agora.saturating_duration_since(desde) >= PRAZO_DO_PAREAMENTO
}

impl Daemon {
    /// Se há um código na tela esperando a comparação do usuário.
    pub(super) const fn aguardando_confirmacao(&self) -> bool {
        matches!(
            self.pareamento,
            Some(Pareamento {
                conferido: false,
                ..
            })
        )
    }

    /// Se há um pareamento em andamento: do código na tela até o fim, com ou sem a resposta do
    /// usuário.
    ///
    /// Não é o mesmo que [`Self::aguardando_confirmacao`]. Depois de "São iguais" o pareamento
    /// ainda espera o outro computador, e tratá-lo como terminado deixava a reconexão discar por
    /// cima e a janela sem saber que não deu (log 25).
    pub(super) const fn pareando(&self) -> bool {
        self.pareamento.is_some()
    }

    /// Desiste do pareamento se ele passou do prazo.
    pub(super) fn vencer_pareamento_se_preciso(&mut self) {
        let Some(pareamento) = self.pareamento else {
            return;
        };
        if !expirou(pareamento.desde, Instant::now()) {
            return;
        }
        if let Some(transporte) = self.transporte_do_par() {
            if pareamento.conferido {
                // O usuário já disse que confere, e é o outro lado que não respondeu. Recusar
                // agora mandaria "códigos diferentes" — o sinal de alguém no meio — por um motivo
                // que não é esse. Desfazer o enlace basta.
                warn!("o pareamento conferido não terminou no prazo");
                transporte.desconectar();
            } else {
                warn!("o código de pareamento expirou sem confirmação");
                // Recusar é o que desmonta o handshake do lado do transporte e avisa o outro
                // computador, para ele também sair da tela de comparação em vez de esperar o
                // próprio prazo vencer.
                transporte.confirmar_pareamento(false);
            }
        }
        self.encerrar_pareamento_sem_sucesso();
    }

    /// O enlace caiu no meio do pareamento: não há mais o que confirmar, nem o que esperar.
    pub(super) fn abandonar_pareamento_pendente(&mut self) {
        if self.pareando() {
            warn!("o enlace caiu no meio do pareamento");
            self.encerrar_pareamento_sem_sucesso();
        }
    }

    /// Limpa o pareamento pendente e tira a interface da tela de comparação.
    pub(super) fn encerrar_pareamento_sem_sucesso(&mut self) {
        self.pareamento = None;
        self.pending_peer = None;
        let _ = self
            .avisos
            .send(Aviso::PareamentoConcluido { sucesso: false });
    }

    /// Esquece o par gravado, e desliga dele na hora.
    ///
    /// Só apagar a chave deixava o enlace e a sessão de pé: a janela seguia em "Conectando…" e o
    /// serviço continuava tentando (log 25). O endereço fica, porque é o candidato que "Procurar"
    /// oferece para parear de novo — e, sem par gravado, ninguém disca para ele sozinho.
    pub(super) fn esquecer_par(&mut self) -> Resposta {
        let mut nova = self.config.clone();
        nova.peers.clear();
        let resposta = self.persistir(nova);
        if resposta != Resposta::Feito {
            return resposta;
        }
        info!("par esquecido pela interface; conexão encerrada");
        // O canal de arquivos com ele cai também: esquecido, ele não recebe mais nada daqui.
        self.arquivos
            .trocar_destino(crate::arquivos::destino(&self.config));
        if self.pareando() {
            self.encerrar_pareamento_sem_sucesso();
        }
        if self.session.phase() != Phase::Offline {
            // Primeiro soltar tudo e avisar o par, pelo enlace que ainda existe; depois derrubá-lo.
            let agora = self.now();
            self.session
                .stop(agora, LinkDown::UserStopped, &mut self.out);
            self.apply_commands();
        }
        self.linked = false;
        if let Some(transporte) = self.transporte_do_par() {
            transporte.desconectar();
        }
        self.last_phase = self.session.phase();
        let _ = self.avisos.send(Aviso::EstadoMudou(self.estado()));
        resposta
    }
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use std::net::SocketAddr;

    use ir_crypto::PublicKey;
    use ir_proto::carrier::Carrier;
    use ir_session::{Input, Role};

    use super::*;
    use crate::actor::bancada::{Bancada, Feito};
    use crate::config::{PinnedPeer, encode_key};
    use ir_transporte::{Endereco, Fato};

    #[test]
    fn um_codigo_recem_mostrado_ainda_vale() {
        let agora = Instant::now();
        assert!(!expirou(agora, agora));
        assert!(!expirou(
            agora,
            agora + PRAZO_DO_PAREAMENTO.saturating_sub(Duration::from_secs(1))
        ));
    }

    #[test]
    fn o_codigo_vence_no_prazo_e_continua_vencido_depois() {
        let desde = Instant::now();
        assert!(expirou(desde, desde + PRAZO_DO_PAREAMENTO));
        assert!(expirou(desde, desde + PRAZO_DO_PAREAMENTO * 2));
    }

    #[test]
    fn um_agora_anterior_ao_inicio_nao_vence_o_codigo() {
        // Saturar e não subtrair: um `agora` que chega antes de `desde` não pode virar um número
        // gigante e declarar vencido um código que acabou de aparecer.
        let agora = Instant::now();
        assert!(!expirou(agora + Duration::from_secs(10), agora));
    }

    #[test]
    fn o_prazo_e_o_mesmo_que_a_interface_promete() {
        let promessa = ir_ipc::Falha::PareamentoExpirou.o_que_fazer();
        assert!(
            promessa.contains("2 minutos"),
            "a interface diz `{promessa}`, e o prazo aqui é {PRAZO_DO_PAREAMENTO:?}"
        );
        assert_eq!(PRAZO_DO_PAREAMENTO, Duration::from_secs(120));
    }

    fn chave() -> PublicKey {
        PublicKey([9; 32])
    }

    fn endereco() -> Endereco {
        Endereco::Rede(
            "10.0.0.135:52526"
                .parse::<SocketAddr>()
                .expect("endereço válido"),
        )
    }

    /// Um serviço que já conhece o endereço do outro computador, sem par gravado.
    fn com_endereco() -> Bancada {
        let mut bancada = Bancada::nova(Role::Server);
        bancada.daemon.peer = Some(endereco());
        bancada
    }

    fn gravar_par(bancada: &mut Bancada) {
        bancada.daemon.config.peers = vec![PinnedPeer {
            pubkey: encode_key(&chave()),
            addr: None,
        }];
    }

    #[test]
    fn sem_par_gravado_o_servico_nao_disca_sozinho() {
        // Discar para parear a cada 3 s punha um código novo na tela do outro computador a cada
        // tentativa, e o código que o usuário estava comparando deixava de valer (log 25).
        let mut bancada = com_endereco();
        bancada.daemon.connect_if_possible();
        bancada.daemon.reconnect_if_needed();
        assert!(
            !Bancada::discou(&bancada.rede.feitos()),
            "parear só quando o usuário pede"
        );
    }

    #[test]
    fn com_par_gravado_o_servico_tenta_reconectar() {
        // Proteção: o que já funcionava continua — um par gravado é procurado sozinho.
        let mut bancada = com_endereco();
        gravar_par(&mut bancada);
        bancada.daemon.reconnect_if_needed();
        let feitos = bancada.rede.feitos();
        assert!(
            feitos
                .iter()
                .any(|feito| matches!(feito, Feito::Conectou { fixada: true, .. })),
            "{feitos:?}"
        );
    }

    #[test]
    fn o_endereco_decide_o_portador_da_reconexao() {
        // O ponto da fiação de portador: um par visto pelo rádio precisa ser procurado pelo
        // rádio. Antes, tudo ia para o socket de rede, qualquer que fosse o portador.
        let mut bancada = Bancada::nova(Role::Server);
        bancada.daemon.peer = Endereco::ler("AC:50:DE:47:EB:28");
        gravar_par(&mut bancada);

        bancada.daemon.reconnect_if_needed();

        assert!(
            Bancada::discou(&bancada.radio.feitos()),
            "a reconexão tinha de sair pelo rádio"
        );
        assert!(!Bancada::discou(&bancada.rede.feitos()), "e não pela rede");
    }

    #[test]
    fn depois_de_conferir_o_codigo_o_servico_nao_disca_por_cima() {
        // O defeito do notebook: "São iguais" chegava ao serviço, e em até 3 s a reconexão
        // discava de novo e desmontava o handshake que esperava a resposta do outro lado.
        let mut bancada = com_endereco();
        bancada.daemon.on_pairing_code([5, 5, 5, 0, 7, 5], chave());
        bancada.daemon.confirmar(true);
        bancada.daemon.reconnect_if_needed();

        let feitos = bancada.rede.feitos();
        assert!(
            feitos
                .iter()
                .any(|feito| matches!(feito, Feito::Confirmou(true)))
        );
        assert!(
            !Bancada::discou(&feitos),
            "discar agora desmontaria o pareamento em andamento: {feitos:?}"
        );
    }

    #[test]
    fn se_o_enlace_cai_depois_de_conferir_a_janela_sai_da_espera() {
        // Sem este aviso, a janela ficava parada na comparação, e o clique parecia não fazer nada.
        let mut bancada = com_endereco();
        let mut avisos = bancada.daemon.avisos.subscribe();
        bancada.daemon.on_pairing_code([3, 3, 4, 5, 8, 9], chave());
        bancada.daemon.confirmar(true);

        bancada.daemon.on_fato_do_transporte(Fato::Caiu {
            portador: Carrier::Udp,
            motivo: "handshake falhou".to_owned(),
        });

        let mut recebidos = Vec::new();
        while let Ok(aviso) = avisos.try_recv() {
            recebidos.push(aviso);
        }
        assert!(
            recebidos
                .iter()
                .any(|aviso| matches!(aviso, Aviso::PareamentoConcluido { sucesso: false })),
            "{recebidos:?}"
        );
    }

    #[test]
    fn um_pareamento_conferido_que_nao_termina_vence_no_prazo_sem_acusar_codigos_diferentes() {
        let mut bancada = com_endereco();
        let mut avisos = bancada.daemon.avisos.subscribe();
        bancada.daemon.on_pairing_code([6, 5, 5, 6, 8, 8], chave());
        bancada.daemon.confirmar(true);
        let _ = bancada.rede.feitos();
        if let Some(pareamento) = bancada.daemon.pareamento.as_mut() {
            pareamento.desde = Instant::now()
                .checked_sub(PRAZO_DO_PAREAMENTO + Duration::from_secs(1))
                .unwrap_or(pareamento.desde);
        }

        bancada.daemon.vencer_pareamento_se_preciso();

        let feitos = bancada.rede.feitos();
        assert!(
            feitos.contains(&Feito::Desconectou),
            "o usuário já disse que confere; recusar agora mandaria \"códigos diferentes\": {feitos:?}"
        );
        let mut recebidos = Vec::new();
        while let Ok(aviso) = avisos.try_recv() {
            recebidos.push(aviso);
        }
        assert!(
            recebidos
                .iter()
                .any(|aviso| matches!(aviso, Aviso::PareamentoConcluido { sucesso: false }))
        );
    }

    #[test]
    fn responder_a_um_codigo_que_ja_nao_vale_e_recusado() {
        // A janela transforma a recusa em explicação, em vez de um clique sem reação.
        let mut bancada = com_endereco();
        assert!(!bancada.daemon.confirmar(true), "não há código na tela");

        bancada.daemon.on_pairing_code([1, 2, 7, 0, 3, 0], chave());
        assert!(bancada.daemon.confirmar(true));
        assert!(
            !bancada.daemon.confirmar(true),
            "o mesmo código não é respondido duas vezes"
        );
    }

    #[test]
    fn esquecer_o_par_encerra_a_conexao_e_nao_tenta_mais() {
        // O defeito do Windows: depois de esquecer, a janela seguia em "Conectando…".
        let mut bancada = com_endereco();
        gravar_par(&mut bancada);
        bancada.daemon.linked = true;
        bancada.daemon.drive(Input::CarrierUp(Carrier::Udp));
        assert_ne!(bancada.daemon.session.phase(), Phase::Offline);
        let _ = bancada.rede.feitos();

        assert_eq!(bancada.daemon.esquecer_par(), Resposta::Feito);

        assert_eq!(bancada.daemon.session.phase(), Phase::Offline);
        let feitos = bancada.rede.feitos();
        assert!(
            feitos.contains(&Feito::Desconectou),
            "o enlace seguro precisa cair: {feitos:?}"
        );
        bancada.daemon.reconnect_if_needed();
        assert!(
            !Bancada::discou(&bancada.rede.feitos()),
            "esquecido, não se procura mais"
        );
        assert!(bancada.daemon.estado().par.is_none());
    }
}
