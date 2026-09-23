//! O adaptador do `ir-net`: a rede local vista como um [`Transporte`].
//!
//! Fino de propósito. Ele não decide nada — traduz o vocabulário do endpoint UDP para o do
//! serviço, e de volta. Toda a máquina de estados do enlace continua no `ir-net`.

use std::net::SocketAddr;
use std::sync::Arc;

use anyhow::{Context, Result};
use ir_crypto::{Identity, PublicKey};
use ir_net::{ConnectMode, Endpoint, NetCommand, NetEvent};
use ir_proto::carrier::Carrier;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};

use crate::{Endereco, Fato, Transporte};

/// A rede local como transporte de entrada.
#[derive(Debug)]
pub struct Rede {
    comandos: UnboundedSender<NetCommand>,
}

impl Rede {
    /// Vincula o socket, sobe o endpoint e passa a relatar em `fatos`.
    ///
    /// # Errors
    ///
    /// Se a porta não puder ser vinculada — quase sempre porque outro programa já a ocupa.
    pub async fn abrir(
        porta: u16,
        identidade: Arc<Identity>,
        fatos: UnboundedSender<Fato>,
    ) -> Result<Self> {
        let escuta: SocketAddr = format!("0.0.0.0:{porta}")
            .parse()
            .context("porta inválida")?;
        let socket = ir_net::bind(escuta)
            .await
            .context("vinculando o socket UDP")?;
        let alca = Endpoint::spawn(socket, identidade);
        tokio::spawn(repassar(alca.events, fatos));
        Ok(Self {
            comandos: alca.commands,
        })
    }
}

impl Transporte for Rede {
    fn portador(&self) -> Carrier {
        Carrier::Udp
    }

    fn conectar(&self, alvo: Endereco, chave: Option<PublicKey>) {
        let Endereco::Rede(peer) = alvo else {
            // Um endereço de rádio não é com este transporte. Ignorar em silêncio é o certo:
            // quem roteia é o ator, e ele não deveria ter mandado — mas errar o destino não pode
            // virar uma conexão para o lugar errado.
            return;
        };
        let mode = match chave {
            Some(fixada) => ConnectMode::Reconnect(fixada),
            None => ConnectMode::Pair,
        };
        let _ = self.comandos.send(NetCommand::Connect { peer, mode });
    }

    fn enviar(&self, bytes: Vec<u8>) {
        let _ = self.comandos.send(NetCommand::SendFrame(bytes));
    }

    fn confirmar_pareamento(&self, conferiu: bool) {
        let _ = self.comandos.send(NetCommand::ConfirmPairing(conferiu));
    }

    fn aceitar_pareamento(&self, aceitar: bool) {
        let _ = self.comandos.send(NetCommand::AcceptPairing(aceitar));
    }

    fn desconectar(&self) {
        let _ = self.comandos.send(NetCommand::Disconnect);
    }
}

/// Repassa os eventos do endpoint como fatos do serviço, até um dos lados ir embora.
async fn repassar(mut eventos: UnboundedReceiver<NetEvent>, fatos: UnboundedSender<Fato>) {
    while let Some(evento) = eventos.recv().await {
        let Some(fato) = fato_de(evento) else {
            continue;
        };
        if fatos.send(fato).is_err() {
            break; // o ator encerrou
        }
    }
}

/// Traduz um evento de rede no fato correspondente.
///
/// Função pura, e é essa a razão de ela existir separada: a tradução é onde se troca um campo
/// pelo outro sem perceber, e aqui ela é verificável sem socket nenhum.
fn fato_de(evento: NetEvent) -> Option<Fato> {
    const PORTADOR: Carrier = Carrier::Udp;
    Some(match evento {
        NetEvent::PairingCode {
            code,
            peer_static,
            peer,
        } => Fato::CodigoDePareamento {
            portador: PORTADOR,
            digitos: code,
            chave_do_par: peer_static,
            de: Endereco::Rede(peer),
        },
        NetEvent::Established { peer_static, peer } => Fato::Estabelecido {
            portador: PORTADOR,
            chave_do_par: peer_static,
            de: Endereco::Rede(peer),
        },
        NetEvent::Frame(bytes) => Fato::Quadro {
            portador: PORTADOR,
            bytes,
        },
        NetEvent::LinkDown(motivo) => Fato::Caiu {
            portador: PORTADOR,
            motivo: motivo.to_owned(),
        },
        NetEvent::Error(mensagem) => Fato::Erro {
            portador: PORTADOR,
            mensagem,
        },
        // O enum é não exaustivo: uma variante nova do `ir-net` não pode derrubar o serviço.
        _ => return None,
    })
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    fn endereco() -> SocketAddr {
        "10.0.0.135:52525".parse().expect("endereço válido")
    }

    #[test]
    fn todo_fato_traduzido_sai_marcado_como_rede() {
        // Se um fato da rede saísse marcado como Bluetooth, o ator alimentaria a sessão com o
        // portador errado — que é a forma mais silenciosa possível deste defeito.
        let eventos = [
            NetEvent::PairingCode {
                code: [1, 2, 3, 4, 5, 6],
                peer_static: PublicKey([9; 32]),
                peer: endereco(),
            },
            NetEvent::Established {
                peer_static: PublicKey([9; 32]),
                peer: endereco(),
            },
            NetEvent::Frame(vec![1, 2, 3]),
            NetEvent::LinkDown("o par sumiu"),
            NetEvent::Error("falhou".to_owned()),
        ];
        for evento in eventos {
            let fato = fato_de(evento).expect("traduz");
            assert_eq!(fato.portador(), Carrier::Udp, "{fato:?}");
        }
    }

    #[test]
    fn o_codigo_e_a_chave_atravessam_sem_troca() {
        // Trocar os dígitos ou a chave no caminho faria o usuário comparar um código que não é o
        // do handshake, ou fixar a identidade errada.
        let fato = fato_de(NetEvent::PairingCode {
            code: [5, 5, 5, 0, 7, 5],
            peer_static: PublicKey([3; 32]),
            peer: endereco(),
        })
        .expect("traduz");
        match fato {
            Fato::CodigoDePareamento {
                digitos,
                chave_do_par,
                de,
                ..
            } => {
                assert_eq!(digitos, [5, 5, 5, 0, 7, 5]);
                assert_eq!(chave_do_par, PublicKey([3; 32]));
                assert_eq!(de, Endereco::Rede(endereco()));
            }
            outro => panic!("esperava o código, veio {outro:?}"),
        }
    }

    #[test]
    fn os_bytes_do_quadro_nao_sao_tocados() {
        let fato = fato_de(NetEvent::Frame(vec![7, 8, 9])).expect("traduz");
        match fato {
            Fato::Quadro { bytes, .. } => assert_eq!(bytes, vec![7, 8, 9]),
            outro => panic!("esperava o quadro, veio {outro:?}"),
        }
    }
}
