//! O enlace com o par: por onde ele fala, e o que fazer com o que ele diz.
//!
//! Saiu do laço central ([`super`]) por tamanho e por assunto. Aqui mora tudo que depende de
//! **qual** portador está em uso — e é justamente o que faltava no serviço: o portador chegava
//! em cada comando e em cada evento, e era descartado no caminho.
//!
//! # A correção que este módulo carrega
//!
//! Antes, o ator alimentava a sessão com `Input::CarrierUp(Carrier::Udp)` fixo, decodificava
//! todo quadro como se fosse UDP e mandava tudo pelo socket de rede. Com um transporte só isso
//! passava despercebido; com dois, a tela diria "Bluetooth" enquanto os bytes iam pela rede — e
//! o limite de quadro conferido seria o errado, porque o teto do rádio é menor
//! ([03, §2](../../../docs/03-protocolo.md)).
//!
//! Agora o portador vem do próprio fato, e a escolha de qual usar continua sendo da sessão.

use ir_crypto::PublicKey;
use ir_proto::carrier::Carrier;
use ir_session::{Input, LinkDown, Phase};
use tracing::{error, info, warn};

use super::Daemon;
use crate::config::{PinnedPeer, encode_key};
use ir_transporte::{Endereco, Fato, Transporte};

impl Daemon {
    /// O transporte de um portador, se ele estiver aberto nesta máquina.
    ///
    /// O rádio pode não existir — sem adaptador, desligado, ou com o canal ocupado. Nesse caso
    /// não há o que devolver, e quem pergunta precisa saber disso em vez de receber a rede no
    /// lugar: degradar é decisão da sessão, não de quem roteia.
    pub(crate) fn transporte(&self, portador: Carrier) -> Option<&dyn Transporte> {
        match portador {
            Carrier::Udp => Some(self.rede.as_ref()),
            Carrier::Rfcomm => self.radio.as_deref(),
            // TCP é o portador de arquivos, e ele não leva quadro de sessão nenhum.
            Carrier::Tcp => None,
        }
    }

    /// O portador que a sessão está usando, ou o da rede enquanto não há sessão.
    pub(super) fn portador_em_uso(&self) -> Carrier {
        self.session.carrier().unwrap_or(Carrier::Udp)
    }

    /// Um fato vindo de um dos transportes.
    pub(super) fn on_fato_do_transporte(&mut self, fato: Fato) {
        let portador = fato.portador();
        match fato {
            Fato::CodigoDePareamento {
                digitos,
                chave_do_par,
                de,
                ..
            } => {
                self.peer = Some(de);
                self.on_pairing_code(digitos, chave_do_par);
            }
            Fato::Estabelecido {
                chave_do_par, de, ..
            } => self.on_established(chave_do_par, de, portador),
            Fato::Quadro { bytes, .. } => self.on_frame(&bytes, portador),
            Fato::Caiu { motivo, .. } => self.on_link_down(&motivo, portador),
            Fato::Erro { mensagem, .. } => warn!(%portador, mensagem, "erro de transporte"),
        }
    }

    /// O enlace caiu. A sessão precisa saber **qual** portador caiu, não que "o portador" caiu.
    fn on_link_down(&mut self, motivo: &str, portador: Carrier) {
        info!(%portador, motivo, "enlace caiu");
        self.linked = false;
        // Um código na tela sem enlace por baixo não tem mais o que confirmar.
        self.abandonar_pareamento_pendente();
        self.drive(Input::CarrierDown {
            carrier: portador,
            reason: LinkDown::TransportFailed,
        });
        self.notar_estado();
    }

    /// O enlace seguro ficou pronto.
    fn on_established(&mut self, chave_do_par: PublicKey, de: Endereco, portador: Carrier) {
        if self.pending_peer.take().is_some() {
            self.pareamento = None;
            self.save_peer(chave_do_par, de);
            let _ = self
                .avisos
                .send(ir_ipc::Aviso::PareamentoConcluido { sucesso: true });
        } else if self.config.first_peer_key() != Some(chave_do_par) {
            // Sem pareamento em curso, só passa quem já está fixado — e exatamente ele.
            //
            // **Não ter chave nenhuma também recusa**, e é essa a parte que faltava. Esquecer um
            // par apaga a chave fixada, e a partir daí aquele computador precisa de código novo e
            // de confirmação visual ([04, §3.4](../../../docs/04-seguranca.md)). Antes, a recusa
            // era "existe fixada **e** difere": com a lista vazia o ramo não disparava, e a
            // máquina reaceitava em silêncio o par que acabara de ser esquecido — bastava ele
            // insistir num `Noise_IK`. Também era isso que mantinha `linked` ligado para sempre,
            // e com ele o laço de sessão que reiniciava a cada segundo.
            warn!(%de, "sem pareamento em curso e sem chave fixada que confira — recusando");
            if let Some(transporte) = self.transporte(portador) {
                transporte.desconectar();
            }
            return;
        }
        self.linked = true;
        self.peer = Some(de);
        // A próxima posição absoluta semeia o ponteiro: o cursor real está onde está, e o modelo
        // da sessão precisa começar no mesmo ponto, senão a primeira travessia dispara errado.
        self.seed_pointer = true;
        info!(%de, %portador, "enlace seguro pronto; iniciando a sessão");
        self.drive(Input::CarrierUp(portador));
        self.notar_estado();
    }

    /// Grava o par, com o endereço por onde ele foi alcançado.
    fn save_peer(&mut self, chave_do_par: PublicKey, de: Endereco) {
        let pinned = PinnedPeer {
            pubkey: encode_key(&chave_do_par),
            // Texto, e é o que permite guardar tanto `ip:porta` quanto endereço de rádio sem
            // mudar o formato do arquivo de configuração.
            addr: Some(de.to_string()),
        };
        self.config.peers = vec![pinned];
        if let Err(error) = self.config.save(&self.data_dir) {
            error!(%error, "não foi possível gravar o par");
        } else {
            info!("par gravado");
        }
        // Arquivos passam a valer com este par agora, e não depois de reiniciar o serviço.
        self.arquivos
            .trocar_destino(crate::arquivos::destino(&self.config));
    }

    /// Um quadro chegou. É decodificado com o limite **deste** portador.
    fn on_frame(&mut self, bytes: &[u8], portador: Carrier) {
        match ir_proto::codec::decode(bytes, portador) {
            Ok(frame) => self.drive(Input::Received {
                carrier: portador,
                frame,
            }),
            Err(error) => warn!(%error, %portador, "quadro recebido malformado"),
        }
    }

    /// Retoma a conexão conforme o que está caído.
    pub(super) fn reconnect_if_needed(&mut self) {
        if self.pareando() {
            return; // no meio de um pareamento, até ele terminar; discar agora o desmontaria
        }
        if self.linked {
            // O enlace seguro está de pé, mas a sessão caiu (silêncio do par). Reinicia a sessão
            // sobre o mesmo enlace: um `Hello` novo, que o par absorve se já estiver de pé.
            if self.session.phase() == Phase::Offline {
                self.drive(Input::CarrierUp(self.portador_em_uso()));
            }
        } else if self.peer.is_some() && self.config.first_peer_key().is_some() {
            // Sem enlace, com endereço e com par gravado: somos o iniciador, e tentamos de novo.
            // Sem par gravado não se disca sozinho — parear é pedido do usuário (log 25).
            self.connect_if_possible();
        }
    }

    /// Reconecta ao par gravado, como iniciador, se houver par e endereço.
    ///
    /// Nunca começa um pareamento. Discar para parear sem o usuário pedir punha um código novo na
    /// tela do outro computador a cada tentativa, e o que ele estava comparando deixava de valer
    /// (log 25). Parear começa pela janela ([`Pedido::IniciarPareamento`](ir_ipc::Pedido)).
    pub(crate) fn connect_if_possible(&self) {
        let Some(chave) = self.config.first_peer_key() else {
            info!("nenhum par gravado; o pareamento começa pela janela");
            return;
        };
        let Some(alvo) = self.peer else {
            info!("sem endereço de par; aguardando conexão de entrada");
            return;
        };
        // O endereço diz por qual portador se fala com ele: `ip:porta` é rede, endereço de rádio
        // é Bluetooth. Não há adivinhação, e não há um portador presumido.
        let portador = alvo.portador();
        let Some(transporte) = self.transporte(portador) else {
            info!(%portador, "o par foi visto por um portador que não está aberto aqui");
            return;
        };
        info!(%alvo, %portador, "conectando ao par");
        transporte.conectar(alvo, Some(chave));
    }
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use std::net::SocketAddr;

    use ir_session::Role;

    use super::*;
    use crate::actor::bancada::{Bancada, Feito};
    use crate::config::{PinnedPeer, encode_key};

    fn chave() -> PublicKey {
        PublicKey([9; 32])
    }

    fn outra_chave() -> PublicKey {
        PublicKey([4; 32])
    }

    fn de_onde() -> Endereco {
        Endereco::Rede(
            "10.0.0.135:52526"
                .parse::<SocketAddr>()
                .expect("endereço válido"),
        )
    }

    fn gravar_par(bancada: &mut Bancada) {
        bancada.daemon.config.peers = vec![PinnedPeer {
            pubkey: encode_key(&chave()),
            addr: None,
        }];
    }

    fn estabeleceu(bancada: &mut Bancada, chave_do_par: PublicKey) {
        bancada.daemon.on_fato_do_transporte(Fato::Estabelecido {
            portador: Carrier::Udp,
            chave_do_par,
            de: de_onde(),
        });
    }

    #[test]
    fn sem_par_gravado_o_servico_nao_aceita_quem_liga() {
        // `docs/04` §3.4: esquecer um par apaga a chave fixada, e a partir daí aquele computador
        // precisa de código novo e confirmação visual. A recusa era "existe fixada **e** difere",
        // então com a lista vazia ninguém era recusado: quem insistisse num `Noise_IK` entrava
        // calado, sem código e sem ninguém confirmar nada (log 27).
        let mut bancada = Bancada::nova(Role::Server);
        assert!(bancada.daemon.config.peers.is_empty(), "nenhum par gravado");

        estabeleceu(&mut bancada, chave());

        assert!(
            !bancada.daemon.linked,
            "sem par gravado, ninguém entra sem passar pelo pareamento"
        );
        assert!(
            bancada.rede.feitos().contains(&Feito::Desconectou),
            "e o enlace tem de cair, senão `linked` fica ligado para sempre"
        );
    }

    #[test]
    fn uma_chave_diferente_da_fixada_e_recusada() {
        let mut bancada = Bancada::nova(Role::Server);
        gravar_par(&mut bancada);

        estabeleceu(&mut bancada, outra_chave());

        assert!(!bancada.daemon.linked, "não é quem está fixado");
    }

    #[test]
    fn com_a_chave_fixada_certa_o_servico_aceita() {
        // Proteção do que já funcionava: uma reconexão legítima continua entrando, senão a
        // correção teria trocado um defeito por outro.
        let mut bancada = Bancada::nova(Role::Server);
        gravar_par(&mut bancada);

        estabeleceu(&mut bancada, chave());

        assert!(bancada.daemon.linked, "a chave confere com a fixada");
    }

    #[test]
    fn com_pareamento_em_curso_o_servico_aceita_e_grava_o_par() {
        // O outro caminho legítimo: o pareamento que o usuário acabou de confirmar.
        let mut bancada = Bancada::nova(Role::Server);
        bancada.daemon.on_pairing_code([1, 2, 3, 4, 5, 6], chave());
        bancada.daemon.confirmar(true);

        estabeleceu(&mut bancada, chave());

        assert!(bancada.daemon.linked);
        assert!(
            !bancada.daemon.config.peers.is_empty(),
            "o par precisa ficar gravado"
        );
    }
}
