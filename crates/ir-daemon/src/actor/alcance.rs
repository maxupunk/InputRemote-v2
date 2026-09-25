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
use tracing::info;

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
            let fixado = self.config.fixado();
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
        if !self.alcance.buscar_agora(agora) {
            return;
        }
        let busca = self.descoberta.localizar(chave.machine_id());
        self.em_fundo_async(|de_fundo| async move {
            if let Some(endereco) = busca.await {
                let _ = de_fundo.send(super::DeFundo::ParAchado(endereco));
            }
        });
    }

    /// A descoberta achou o par na rede.
    pub(crate) fn on_par_achado(&mut self, endereco: SocketAddr) {
        info!(%endereco, "par achado na rede");
        self.alcance.anotar(Endereco::Rede(endereco));
        self.atualizar_destino_dos_arquivos();
        self.discar_o_que_falta();
    }

    /// Conta ao canal de arquivos onde o par está agora, pelo [`ir_transporte::Alcance`].
    ///
    /// O canal de arquivos lia o endereço da configuração uma vez, e fazia a própria busca na rede:
    /// quando o serviço achava o par em outro endereço — um DHCP que mudou —, a entrada ia para o
    /// endereço novo e os arquivos continuavam discando o velho. O `Alcance` é a fonte de onde o par
    /// está; o que a configuração diz entra nele na subida.
    pub(crate) fn atualizar_destino_dos_arquivos(&self) {
        let mut destino = crate::arquivos::destino(&self.config);
        if let Some(rede) = self.alcance.endereco(Carrier::Udp) {
            destino.alvo = Some(rede);
        }
        self.arquivos.trocar_destino(destino);
    }

    /// O par contou o endereço do rádio dele: guardar — também no disco, para a próxima subida já
    /// discar o Bluetooth — e discar se ainda não há enlace.
    pub(crate) fn on_radio_do_par(&mut self, radio: RadioAddress) {
        let endereco = Endereco::do_radio(radio);
        let novo = self.alcance.endereco(Carrier::Rfcomm) != Some(endereco);
        if novo {
            info!(%endereco, "o par contou o endereço do rádio dele");
            self.alcance.anotar(endereco);
        }
        // Sem esperar: isto chega com a sessão recém-estabelecida, e o ponteiro já anda.
        if novo && !self.config.peers.is_empty() {
            self.gravar_ja(|config| {
                if let Some(par) = config.peers.first_mut() {
                    par.radio = Some(endereco.to_string());
                }
            });
        }
        self.discar_o_que_falta();
    }

    /// O rádio abriu depois da subida — o canal estava ocupado pelo serviço anterior.
    ///
    /// Entra como se tivesse aberto na subida: a busca passa a listar os pareados, a sessão conta o
    /// endereço ao par, e o que faltava discar é discado.
    pub(crate) fn on_radio_tardio(&mut self, aberto: ir_transporte::RadioAberto) {
        info!("o rádio Bluetooth abriu depois da subida; a rota dupla volta");
        self.radio = Some(aberto.transporte);
        self.radio_proprio = aberto.proprio;
        self.descoberta.adotar_pareados(aberto.pareados);
        self.anunciar_radio_proprio();
        // O rádio novo não ouviu a decisão sobre pedidos de pareamento de fora.
        self.abertura_anunciada = None;
        self.anunciar_abertura();
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
mod testes;
