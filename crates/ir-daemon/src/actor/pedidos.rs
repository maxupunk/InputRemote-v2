//! A face de IPC do ator: traduzir pedidos da interface em ação, e o estado interno no
//! [`Estado`] publicado.
//!
//! Fica separado do laço central ([`super`]) por tamanho e por assunto: aqui é só a fronteira
//! com a interface — nenhuma decisão de sessão acontece neste arquivo, só a tradução entre o
//! vocabulário de dentro e o de fora. As `impl` são do mesmo [`Daemon`]; um submódulo enxerga
//! os campos privados do pai, então nada precisou virar público para isto morar aqui.

use std::net::SocketAddr;

use ir_ipc::{
    Aviso, Borda, Candidato, Estado, Falha, LinkState, Maquina, Nivel, Nome, Papel, ParConhecido,
    Pedido, Portador, Recursos, Resposta,
};
use ir_net::{ConnectMode, NetCommand};
use ir_proto::carrier::Carrier;
use ir_proto::screens::Edge;
use ir_session::{Phase, Role};
use tracing::{error, info};

use super::Daemon;
use crate::config::{Config, decode_key};
use crate::ipc::PedidoRecebido;

impl Daemon {
    /// Um pedido da interface: traduz, age, e devolve a resposta pelo caminho de volta.
    pub(super) fn on_pedido(&mut self, recebido: PedidoRecebido) {
        let PedidoRecebido { pedido, responder } = recebido;
        let resposta = self.tratar(pedido);
        let _ = responder.send(resposta);
    }

    /// A ação de cada pedido. A autoridade é conferida no transporte, não aqui.
    fn tratar(&mut self, pedido: Pedido) -> Resposta {
        match pedido {
            Pedido::Estado => Resposta::Estado(self.estado()),
            Pedido::Acompanhar => Resposta::Feito,
            Pedido::Procurar => {
                self.anunciar_candidato();
                Resposta::Feito
            }
            Pedido::IniciarPareamento { candidato } => self.iniciar_pareamento(&candidato),
            Pedido::ConfirmarPareamento { conferiu } => {
                self.confirmar(conferiu);
                Resposta::Feito
            }
            Pedido::Encerrar => {
                let _ = self.net.send(NetCommand::Disconnect);
                Resposta::Feito
            }
            Pedido::EsquecerPar { .. } => self.esquecer_par(),
            Pedido::FixarPortador(portador) => {
                self.session
                    .pin_carrier(portador.map(Portador::no_protocolo));
                Resposta::Feito
            }
            // As duas trocas valem na hora: a sessão é refeita com o valor novo (`super::papel`).
            Pedido::DefinirBorda(borda) => self.trocar_borda(borda.no_protocolo()),
            Pedido::DefinirPapel(papel) => self.trocar_papel(role_de(papel)),
            Pedido::Diagnostico => Resposta::Diagnostico(self.diagnostico()),
            // A tela de bloqueio é N2: depende do agente no desktop seguro, que ainda não entra.
            // O curinga cobre também variantes futuras do contrato ainda não tratadas aqui.
            _ => Resposta::Falha(Falha::ForaDeContexto),
        }
    }

    /// Anuncia o par configurado como candidato, no lugar da descoberta (ainda não ligada).
    fn anunciar_candidato(&self) {
        let mut candidatos = Vec::new();
        if let Some(addr) = self.peer_addr {
            candidatos.push(Candidato {
                rotulo: format!("Computador em {addr}"),
                endereco: addr.to_string(),
                portador: Portador::RedeLocal,
            });
        }
        let _ = self
            .avisos
            .send(Aviso::CandidatosEncontrados { candidatos });
    }

    /// Começa a parear com o endereço escolhido.
    fn iniciar_pareamento(&mut self, candidato: &str) -> Resposta {
        let Ok(addr) = candidato.parse::<SocketAddr>() else {
            return Resposta::Falha(Falha::ForaDeContexto);
        };
        self.peer_addr = Some(addr);
        let _ = self.net.send(NetCommand::Connect {
            peer: addr,
            mode: ConnectMode::Pair,
        });
        Resposta::Feito
    }

    /// Esquece o par gravado.
    fn esquecer_par(&mut self) -> Resposta {
        let mut nova = self.config.clone();
        nova.peers.clear();
        let resposta = self.persistir(nova);
        if resposta == Resposta::Feito {
            info!("par esquecido pela interface");
        }
        resposta
    }

    /// Grava a configuração nova e **só então** a adota.
    ///
    /// Um ponto só para toda ação que grava. A ordem é o que importa: se a gravação falha, nem o
    /// arquivo nem a memória mudam, e os dois nunca divergem — antes a memória mudava primeiro, e
    /// uma gravação que falhasse deixava o serviço usando um valor que o próximo reinício perderia.
    pub(super) fn persistir(&mut self, nova: Config) -> Resposta {
        match nova.save(&self.data_dir) {
            Ok(()) => {
                self.config = nova;
                Resposta::Feito
            }
            Err(erro) => {
                error!(%erro, "não foi possível gravar a configuração");
                Resposta::Falha(Falha::Interna)
            }
        }
    }

    /// O relatório de diagnóstico, já pronto para copiar.
    fn diagnostico(&self) -> String {
        format!(
            "papel: {:?}\nfase: {}\nenlace seguro: {}\npares gravados: {}\nendereço do par: {:?}",
            self.session.role(),
            self.session.phase(),
            self.linked,
            self.config.peers.len(),
            self.peer_addr,
        )
    }

    /// O estado corrente, no vocabulário publicado da interface.
    pub(super) fn estado(&self) -> Estado {
        Estado {
            enlace: link_state(self.session.phase()),
            papel: papel_de(self.session.role()),
            borda_do_par: borda_de(self.edge),
            esta_maquina: self.machine,
            este_nome: self.nome.clone(),
            par: self.par_conhecido(),
            portador: self.session.carrier().map(portador_de),
            portador_fixado: None,
            motivo_do_portador: self
                .session
                .carrier()
                .map(|_| ir_ipc::MotivoDoPortador::RedeComoAlternativa),
            latencia: None,
            nivel_privilegiado: Nivel::SoDesbloqueado,
            // Pronto para digitar: ou o agente está de pé (Windows), ou o serviço injeta direto
            // por `uinput` (Linux). Sem um dos dois, nada é digitado nesta máquina.
            agente_pronto: self.agente_pronto || self.injector.is_some() || self.capturer.is_some(),
            bloqueio_permitido: false,
            ultima_queda: None,
        }
    }

    /// O par gravado, resumido para a interface.
    fn par_conhecido(&self) -> Option<ParConhecido> {
        let pinned = self.config.peers.first()?;
        let bytes: [u8; 16] = decode_key(&pinned.pubkey)
            .and_then(|chave| chave.0.get(..16).and_then(|fatia| fatia.try_into().ok()))
            .unwrap_or([0u8; 16]);
        Some(ParConhecido {
            maquina: Maquina(bytes),
            nome: Nome::coagido("computador pareado"),
            recursos: Recursos::default(),
            conectado: self.linked,
        })
    }
}

/// A fase da sessão, traduzida para o enlace que a interface mostra.
const fn link_state(phase: Phase) -> LinkState {
    match phase {
        Phase::Offline => LinkState::Desconectado,
        Phase::Handshaking => LinkState::Conectando,
        Phase::Ready => LinkState::Pronto,
        Phase::Engaged => LinkState::EmUso,
    }
}

/// O papel da sessão, no vocabulário da interface.
const fn papel_de(role: Role) -> Papel {
    match role {
        Role::Server => Papel::Servidor,
        Role::Client => Papel::Cliente,
    }
}

/// A borda do protocolo, no vocabulário da interface.
const fn borda_de(edge: Edge) -> Borda {
    match edge {
        Edge::Left => Borda::Esquerda,
        Edge::Right => Borda::Direita,
        Edge::Top => Borda::Acima,
        Edge::Bottom => Borda::Abaixo,
    }
}

/// O portador do protocolo, no vocabulário da interface.
const fn portador_de(carrier: Carrier) -> Portador {
    match carrier {
        Carrier::Rfcomm => Portador::Bluetooth,
        Carrier::Udp => Portador::RedeLocal,
        Carrier::Tcp => Portador::RedeDeArquivos,
    }
}

/// O papel da interface, no vocabulário da sessão.
const fn role_de(papel: Papel) -> Role {
    match papel {
        Papel::Servidor => Role::Server,
        Papel::Cliente => Role::Client,
    }
}
