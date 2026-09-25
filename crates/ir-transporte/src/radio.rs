//! O adaptador do `ir-bt`: o rádio Bluetooth visto como um [`Transporte`].
//!
//! Gêmeo de [`rede`](crate::rede), e de propósito: os dois endpoints têm comandos e eventos com
//! a mesma forma, então os dois adaptadores têm a mesma forma. É isso que permite ao ator ter um
//! caminho de código só para os dois portadores.
//!
//! # Abrir pode falhar, e a falha é informação
//!
//! Diferente da rede, aqui **não abrir é um resultado esperado**: pode não haver rádio, ele pode
//! estar desligado, ou o canal do produto pode estar ocupado. O erro que sai daqui é o que faz a
//! sessão saber que `Carrier::Rfcomm` não está disponível — e é a partir daí que a política
//! única do `ir-session` degrada para a rede, com o motivo aparecendo na tela.

use std::sync::Arc;

use ir_bt::{BtCommand, BtEvent, ConnectMode, Endpoint};
use ir_crypto::{Identity, PublicKey};
use ir_proto::carrier::Carrier;
use ir_proto::ids::RadioAddress;
use tokio::sync::mpsc::UnboundedSender;

use crate::{Endereco, Fato, Transporte, repassar};

/// O rádio Bluetooth como transporte de entrada.
#[derive(Debug)]
pub struct Radio {
    comandos: UnboundedSender<BtCommand>,
    /// O rádio do sistema, guardado para a busca poder listar os pareados.
    sistema: Arc<ir_bt::RadioDoSistema>,
}

impl Radio {
    /// Abre o rádio, sobe o endpoint e passa a relatar em `fatos`.
    ///
    /// # Errors
    ///
    /// [`ir_bt::BtError::SemRadio`] se não houver rádio utilizável — o caso em que o produto
    /// **não** deve insistir. Qualquer outro erro se o canal do produto não puder ser aberto.
    pub fn abrir(identidade: Arc<Identity>, fatos: UnboundedSender<Fato>) -> ir_bt::Result<Self> {
        let radio = Arc::new(ir_bt::abrir_radio()?);
        let alca = Endpoint::spawn(Arc::clone(&radio), identidade);
        tokio::spawn(repassar(alca.events, fatos, fato_de));
        Ok(Self {
            comandos: alca.commands,
            sistema: radio,
        })
    }

    /// O endereço do rádio desta máquina, no vocabulário do protocolo — é o que a sessão conta ao
    /// par em `Control::Reach`.
    #[must_use]
    pub fn endereco_proprio(&self) -> Option<RadioAddress> {
        ir_bt::Radio::endereco_local(self.sistema.as_ref())
            .map(|endereco| RadioAddress(endereco.bytes()))
    }

    /// Uma alça para listar os dispositivos pareados no sistema, que pode ir para outra tarefa.
    #[must_use]
    pub fn pareados(&self) -> Pareados {
        Pareados(Arc::clone(&self.sistema))
    }
}

/// Lista os pareados do sistema, fora da tarefa do ator.
#[derive(Debug, Clone)]
pub struct Pareados(Arc<ir_bt::RadioDoSistema>);

impl Pareados {
    /// Os dispositivos pareados; vazio se o rádio não disser.
    pub async fn listar(&self) -> Vec<ir_bt::Dispositivo> {
        ir_bt::Radio::pareados(self.0.as_ref())
            .await
            .unwrap_or_default()
    }
}

impl Transporte for Radio {
    fn portador(&self) -> Carrier {
        Carrier::Rfcomm
    }

    fn conectar(&self, alvo: Endereco, chave: Option<PublicKey>) {
        let Endereco::Radio(peer) = alvo else {
            // Um endereço de rede não é com este transporte.
            return;
        };
        let _ = self.comandos.send(BtCommand::Connect {
            peer,
            mode: ConnectMode::de_chave(chave),
        });
    }

    fn enviar(&self, bytes: Vec<u8>) {
        let _ = self.comandos.send(BtCommand::quadro(bytes));
    }

    fn confirmar_pareamento(&self, conferiu: bool) {
        let _ = self.comandos.send(BtCommand::ConfirmPairing(conferiu));
    }

    fn aceitar_pareamento(&self, aceitar: bool) {
        let _ = self.comandos.send(BtCommand::AcceptPairing(aceitar));
    }

    fn desconectar(&self) {
        let _ = self.comandos.send(BtCommand::Disconnect);
    }
}

/// Traduz um evento do rádio no fato correspondente.
///
/// Pura, como a gêmea da rede, e pelo mesmo motivo: é onde se troca um campo pelo outro sem
/// perceber, e aqui dá para verificar sem rádio nenhum.
fn fato_de(evento: BtEvent) -> Option<Fato> {
    const PORTADOR: Carrier = Carrier::Rfcomm;
    Some(match evento {
        BtEvent::PairingCode {
            code,
            peer_static,
            peer,
        } => Fato::CodigoDePareamento {
            portador: PORTADOR,
            digitos: code,
            chave_do_par: peer_static,
            de: Endereco::Radio(peer),
        },
        BtEvent::Established { peer_static, peer } => Fato::Estabelecido {
            portador: PORTADOR,
            chave_do_par: peer_static,
            de: Endereco::Radio(peer),
        },
        BtEvent::Frame(bytes) => Fato::Quadro {
            portador: PORTADOR,
            bytes,
        },
        BtEvent::LinkDown(motivo) => Fato::Caiu {
            portador: PORTADOR,
            motivo: motivo.to_owned(),
        },
        BtEvent::Error(mensagem) => Fato::Erro {
            portador: PORTADOR,
            mensagem,
        },
        BtEvent::RadioLost(motivo) => Fato::Perdido {
            portador: PORTADOR,
            motivo,
        },
        // O enum é não exaustivo: uma variante nova do `ir-bt` não pode derrubar o serviço.
        _ => return None,
    })
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::panic)]
mod tests {
    use ir_bt::BdAddr;

    use super::*;

    const PAR: BdAddr = BdAddr([0xAC, 0x50, 0xDE, 0x47, 0xEB, 0x28]);

    #[test]
    fn todo_fato_traduzido_sai_marcado_como_bluetooth() {
        // A propriedade que faltava no serviço: o portador do fato é o portador de verdade, e
        // não o `Carrier::Udp` fixo que o ator usava quando só havia um transporte.
        let eventos = [
            BtEvent::PairingCode {
                code: [1, 2, 3, 4, 5, 6],
                peer_static: PublicKey([9; 32]),
                peer: PAR,
            },
            BtEvent::Established {
                peer_static: PublicKey([9; 32]),
                peer: PAR,
            },
            BtEvent::Frame(vec![1, 2, 3]),
            BtEvent::LinkDown("o par encerrou o canal"),
            BtEvent::Error("falhou".to_owned()),
        ];
        for evento in eventos {
            let fato = fato_de(evento).expect("traduz");
            assert_eq!(fato.portador(), Carrier::Rfcomm, "{fato:?}");
        }
    }

    #[test]
    fn o_endereco_do_par_chega_como_endereco_de_radio() {
        // É o que o serviço grava para reconectar depois. Se virasse endereço de rede, a
        // reconexão discaria pelo portador errado — ou não discaria.
        let fato = fato_de(BtEvent::Established {
            peer_static: PublicKey([3; 32]),
            peer: PAR,
        })
        .expect("traduz");
        match fato {
            Fato::Estabelecido { de, .. } => {
                assert_eq!(de, Endereco::Radio(PAR));
                assert_eq!(de.portador(), Carrier::Rfcomm);
                assert_eq!(de.to_string(), "AC:50:DE:47:EB:28");
            }
            outro => panic!("esperava o estabelecido, veio {outro:?}"),
        }
    }

    #[test]
    fn o_motivo_da_queda_atravessa_inteiro() {
        // "o par encerrou o canal" e "o quadro não abriu" são causas diferentes, e o `ir-bt` já
        // as distingue. Perder essa distinção aqui apagaria o trabalho feito lá.
        let fato = fato_de(BtEvent::LinkDown("o par encerrou o canal")).expect("traduz");
        match fato {
            Fato::Caiu { motivo, .. } => assert_eq!(motivo, "o par encerrou o canal"),
            outro => panic!("esperava a queda, veio {outro:?}"),
        }
    }
}
