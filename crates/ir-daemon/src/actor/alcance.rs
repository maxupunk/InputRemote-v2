//! O que o serviço faz com o [`Alcance`]: discar o que falta para a rota dupla, achar o par na rede,
//! guardar o rádio que o par contou ([ADR-0012](../../../../docs/adr/0012-rota-dupla.md)).
//!
//! Quem decide **usar** os dois portadores é a sessão; aqui só se garante que, quando os dois
//! existem, os dois estão de pé.

use std::net::SocketAddr;
use std::time::Instant;

use ir_proto::carrier::Carrier;
use ir_proto::ids::RadioAddress;
use ir_session::Input;
use tracing::{info, warn};

use super::Daemon;
use crate::config::Config;
use ir_transporte::{Alcance, Endereco};

/// O que a configuração sabe: o `peer_addr` e os endereços gravados (o `addr` antigo vale os dois).
pub(crate) fn da_configuracao(config: &Config) -> Alcance {
    let par = config.peers.first();
    let gravados = [
        par.and_then(|p| p.addr.as_deref()),
        par.and_then(|p| p.radio.as_deref()),
    ];
    Alcance::dos_textos(config.peer_addr.as_deref(), gravados)
}

impl Daemon {
    /// Se há enlace seguro de pé por algum portador. Distinto de a sessão estar estabelecida.
    pub(crate) fn linked(&self) -> bool {
        self.alcance.algum_de_pe()
    }

    /// Disca, com a chave fixada, cada portador sem enlace e com endereço — só o fixado, se houver.
    /// Nunca começa um pareamento (log 25); a vez de discar é do transporte ([`ir_crypto::turno`]).
    pub(crate) fn discar_o_que_falta(&mut self) {
        let Some(chave) = self.config.first_peer_key() else {
            return;
        };
        let agora = Instant::now();
        for portador in [Carrier::Rfcomm, Carrier::Udp] {
            let fixado = self.portador_fixado.map(ir_ipc::Portador::no_protocolo);
            if fixado.is_some_and(|fixado| fixado != portador)
                || !self.alcance.pode_discar(portador, agora)
            {
                continue;
            }
            let Some(alvo) = self.alcance.endereco(portador) else {
                if portador == Carrier::Udp {
                    self.procurar_o_par_na_rede(chave, agora);
                }
                continue;
            };
            let Some(transporte) = self.transporte(portador) else {
                continue; // sem rádio nesta máquina, por exemplo
            };
            info!(%alvo, %portador, "discando o par");
            transporte.conectar(alvo, Some(chave));
            self.alcance.discou(portador, agora);
        }
    }

    /// Procura o par na rede pela chave fixada, fora do ator; responde em [`Self::on_par_achado`].
    fn procurar_o_par_na_rede(&mut self, chave: ir_crypto::PublicKey, agora: Instant) {
        // Sem runtime (os testes síncronos do ator) não há busca; o resto do serviço segue igual.
        let Ok(runtime) = tokio::runtime::Handle::try_current() else {
            return;
        };
        if !self.alcance.buscar_agora(agora) {
            return;
        }
        let busca = self.descoberta.localizar(crate::machine_id_of(&chave));
        let achados = self.achados.clone();
        runtime.spawn(async move {
            if let Some(endereco) = busca.await {
                let _ = achados.send(endereco);
            }
        });
    }

    /// A descoberta achou o par na rede.
    pub(crate) fn on_par_achado(&mut self, endereco: SocketAddr) {
        info!(%endereco, "par achado na rede");
        self.alcance.anotar(Endereco::Rede(endereco));
        self.discar_o_que_falta();
    }

    /// O par contou o endereço do rádio dele: guardar — também no disco, para a próxima subida já
    /// discar o Bluetooth — e discar se ainda não há enlace.
    pub(crate) fn on_radio_do_par(&mut self, radio: RadioAddress) {
        let endereco = Endereco::do_radio(radio);
        if self.alcance.endereco(Carrier::Rfcomm) != Some(endereco) {
            info!(%endereco, "o par contou o endereço do rádio dele");
            self.alcance.anotar(endereco);
            if let Some(par) = self.config.peers.first_mut() {
                par.radio = Some(endereco.to_string());
                if let Err(erro) = self.config.save(&self.data_dir) {
                    warn!(%erro, "não foi possível gravar o endereço do rádio do par");
                }
            }
        }
        self.discar_o_que_falta();
    }

    /// Conta à sessão o rádio daqui, para ela anunciá-lo ao par — na subida e em toda sessão nova.
    pub(crate) fn anunciar_radio_proprio(&mut self) {
        if let Some(radio) = self.radio_proprio {
            self.drive(Input::LocalRadio(radio));
        }
    }

    /// Recomeça a sessão sobre os enlaces de pé: o primeiro `CarrierUp` começa o aperto de mão, o
    /// outro entra na rota quando a sessão ficar de pé. Antes, recomeçar dizia sempre "rede".
    pub(crate) fn retomar_sessao(&mut self) {
        let de_pe: Vec<Carrier> = self.alcance.de_pe_agora().collect();
        for portador in de_pe {
            self.drive(Input::CarrierUp(portador));
        }
    }

    /// Derruba o enlace por todos os portadores.
    pub(crate) fn desconectar_todos(&mut self) {
        for portador in [Carrier::Rfcomm, Carrier::Udp] {
            if self.alcance.de_pe(portador)
                && let Some(transporte) = self.transporte(portador)
            {
                transporte.desconectar();
            }
        }
        self.alcance.derrubar_todos();
    }
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod testes;
