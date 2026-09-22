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
            Fato::Erro { mensagem, .. } => {
                warn!(%portador, mensagem, "erro de transporte");
                self.discagem_com_erro(portador);
                // Sem enlace de pé por ele, o erro é de uma discagem que não deu em nada: a próxima
                // rodada pode tentar de novo. Pela rede, o endereço aprendido pode ter envelhecido.
                if !self.alcance.de_pe(portador) {
                    if portador == Carrier::Udp {
                        self.alcance.rede_falhou();
                    } else {
                        self.alcance.caiu(portador);
                    }
                }
            }
        }
    }

    /// O enlace caiu. A sessão precisa saber **qual** portador caiu, não que "o portador" caiu.
    fn on_link_down(&mut self, motivo: &str, portador: Carrier) {
        info!(%portador, motivo, "enlace caiu");
        self.alcance.caiu(portador);
        // Um código na tela sem enlace por baixo não tem mais o que confirmar — mas só o enlace
        // do pareamento o sustenta; a queda de outro portador não o desfaz.
        if self.peer.map(Endereco::portador) == Some(portador) {
            self.abandonar_pareamento_pendente();
        }
        self.drive(Input::CarrierDown {
            carrier: portador,
            reason: LinkDown::TransportFailed,
        });
        self.notar_estado();
    }

    /// O enlace seguro ficou pronto.
    fn on_established(&mut self, chave_do_par: PublicKey, de: Endereco, portador: Carrier) {
        if let Some(pendente) = self.pending_peer {
            // Com um código na tela, só o enlace do pareamento o conclui: o mesmo portador e a mesma
            // chave. Outro enlace nesse intervalo — o outro portador da rota dupla discando por
            // conta própria, ou outro computador — levaria a chave sem a confirmação do usuário.
            if pendente != chave_do_par || self.peer.map(Endereco::portador) != Some(portador) {
                warn!(%de, %portador, "enlace alheio ao pareamento em curso — recusando");
                if let Some(transporte) = self.transporte(portador) {
                    transporte.desconectar();
                }
                return;
            }
            self.pending_peer = None;
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
        self.alcance.subiu(portador);
        self.alcance.anotar(de);
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
            // Par novo, identidade nova: o rádio dele chega de novo pelo `Control::Reach`.
            radio: None,
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
        if self.linked() && self.session.phase() == Phase::Offline {
            // O enlace seguro está de pé, mas a sessão caiu (silêncio do par). Reinicia a sessão
            // sobre os mesmos enlaces: um `Hello` novo, que o par absorve se já estiver de pé.
            self.retomar_sessao();
        }
        // Todo portador sem enlace e com endereço conhecido é discado — também com a sessão de pé
        // por outro: é assim que a rota dupla se forma e se refaz (ADR-0012). Sem par gravado não
        // se disca sozinho — parear é pedido do usuário (log 25).
        self.discar_o_que_falta();
    }

    /// Reconecta ao par gravado, como iniciador, por todo portador de que se sabe o endereço.
    ///
    /// Nunca começa um pareamento. Discar para parear sem o usuário pedir punha um código novo na
    /// tela do outro computador a cada tentativa, e o que ele estava comparando deixava de valer
    /// (log 25). Parear começa pela janela ([`Pedido::IniciarPareamento`](ir_ipc::Pedido)).
    pub(crate) fn connect_if_possible(&mut self) {
        if self.config.first_peer_key().is_none() {
            info!("nenhum par gravado; o pareamento começa pela janela");
            return;
        }
        self.discar_o_que_falta();
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
            radio: None,
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
            !bancada.daemon.linked(),
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

        assert!(!bancada.daemon.linked(), "não é quem está fixado");
    }

    #[test]
    fn com_a_chave_fixada_certa_o_servico_aceita() {
        // Proteção do que já funcionava: uma reconexão legítima continua entrando, senão a
        // correção teria trocado um defeito por outro.
        let mut bancada = Bancada::nova(Role::Server);
        gravar_par(&mut bancada);

        estabeleceu(&mut bancada, chave());

        assert!(bancada.daemon.linked(), "a chave confere com a fixada");
    }

    #[test]
    fn com_pareamento_em_curso_o_servico_aceita_e_grava_o_par() {
        // O outro caminho legítimo: o pareamento que o usuário acabou de confirmar.
        // O código chega como na produção, pelo transporte: é ele que diz por onde se pareia, e só
        // o enlace desse portador conclui o pareamento.
        let mut bancada = Bancada::nova(Role::Server);
        bancada
            .daemon
            .on_fato_do_transporte(Fato::CodigoDePareamento {
                portador: Carrier::Udp,
                digitos: [1, 2, 3, 4, 5, 6],
                chave_do_par: chave(),
                de: de_onde(),
            });
        bancada.daemon.confirmar(true);

        estabeleceu(&mut bancada, chave());

        assert!(bancada.daemon.linked());
        assert!(
            !bancada.daemon.config.peers.is_empty(),
            "o par precisa ficar gravado"
        );
    }
}
