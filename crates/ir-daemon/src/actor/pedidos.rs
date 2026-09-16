//! A face de IPC do ator: traduzir pedidos da interface em ação, e o estado interno no
//! [`Estado`] publicado.
//!
//! Fica separado do laço central ([`super`]) por tamanho e por assunto: aqui é só a fronteira
//! com a interface — nenhuma decisão de sessão acontece neste arquivo, só a tradução entre o
//! vocabulário de dentro e o de fora. As `impl` são do mesmo [`Daemon`]; um submódulo enxerga
//! os campos privados do pai, então nada precisou virar público para isto morar aqui.

use ir_ipc::{
    Aviso, Borda, Candidato, Estado, Falha, LinkState, Maquina, MotivoDoPortador, Nivel, Nome,
    Papel, ParConhecido, Pedido, Portador, Recursos, Resposta,
};
use ir_proto::carrier::Carrier;
use ir_proto::screens::Edge;
use ir_session::{Phase, Role};
use tracing::error;

use super::Daemon;
use crate::config::{Config, decode_key};
use crate::ipc::PedidoRecebido;
use ir_transporte::Endereco;

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
                if !self.confirmar(conferiu) {
                    // O código já não vale: venceu, ou o enlace caiu. Dizer isso, e não "feito".
                    Resposta::Falha(Falha::PareamentoInterrompido)
                } else if conferiu {
                    Resposta::Feito
                } else {
                    Resposta::Falha(Falha::CodigosDiferentes)
                }
            }
            Pedido::Encerrar => {
                if let Some(transporte) = self.transporte_do_par() {
                    transporte.desconectar();
                }
                Resposta::Feito
            }
            Pedido::EsquecerPar { .. } => self.esquecer_par(),
            Pedido::FixarPortador(portador) => {
                // Guardado dos dois lados: a sessão precisa dele para desligar a degradação, e a
                // interface precisa vê-lo de volta para dizer "fixado nas preferências" em vez de
                // inventar um motivo.
                self.portador_fixado = portador;
                self.session
                    .pin_carrier(portador.map(Portador::no_protocolo));
                Resposta::Feito
            }
            // As duas trocas valem na hora (`super::papel`): a de papel refaz a sessão, e a de borda
            // só a ajusta — e só no servidor, que é quem decide a borda.
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
        if let Some(endereco) = self.peer {
            let portador = Portador::from(endereco.portador());
            candidatos.push(Candidato {
                rotulo: format!("Computador em {endereco}"),
                endereco: endereco.to_string(),
                // O portador vem do próprio endereço, e não de um presumido: um endereço de
                // rádio na lista precisa aparecer como Bluetooth, senão a tela promete uma coisa
                // e o serviço faz outra.
                portador,
            });
        }
        let _ = self
            .avisos
            .send(Aviso::CandidatosEncontrados { candidatos });
    }

    /// Começa a parear com o endereço escolhido.
    ///
    /// O texto pode ser um `ip:porta` ou um endereço de rádio, e é ele que decide o portador. É o
    /// que permite o Bluetooth entrar sem vocabulário novo na interface.
    fn iniciar_pareamento(&mut self, candidato: &str) -> Resposta {
        let Some(alvo) = Endereco::ler(candidato) else {
            return Resposta::Falha(Falha::ForaDeContexto);
        };
        if self.transporte(alvo.portador()).is_none() {
            // Pediram para parear por um portador que não está aberto nesta máquina — sem rádio,
            // por exemplo. Dizer isso é melhor que ficar em silêncio esperando um código.
            return Resposta::Falha(Falha::ForaDeContexto);
        }
        // O endereço é guardado **antes** de discar: é ele que diz por onde responder à
        // comparação de códigos, e a resposta do par pode chegar antes da próxima linha.
        self.peer = Some(alvo);
        if let Some(transporte) = self.transporte(alvo.portador()) {
            transporte.conectar(alvo, None);
        }
        Resposta::Feito
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
            "papel: {:?}\nfase: {}\nenlace seguro: {}\npares gravados: {}\nendereço do par: {}\n\
             rádio Bluetooth: {}\nportador em uso: {}",
            self.session.role(),
            self.session.phase(),
            self.linked,
            self.config.peers.len(),
            self.peer
                .map_or_else(|| "nenhum".to_owned(), |par| par.to_string()),
            if self.radio.is_some() {
                "aberto"
            } else {
                "indisponível"
            },
            // O nome técnico, que é o que serve num diagnóstico.
            self.session
                .carrier()
                .map_or("nenhum", |portador| Portador::from(portador).nome_tecnico()),
        )
    }

    /// Por que o portador em uso foi escolhido.
    ///
    /// Antes isto era `RedeComoAlternativa` fixo, o que fazia a tela dizer "Bluetooth
    /// indisponível; usando a rede local" **mesmo com o rádio ligado e conectado dos dois
    /// lados** — a frase que deu origem a este trabalho. A escolha é da sessão
    /// ([`CarrierSet::pick_input_carrier`](ir_session::CarrierSet)); aqui só se conta qual foi.
    fn motivo_do_portador(&self) -> Option<MotivoDoPortador> {
        let portador = self.session.carrier()?;
        Some(if self.portador_fixado.is_some() {
            MotivoDoPortador::FixadoPeloUsuario
        } else if portador == Carrier::Rfcomm {
            MotivoDoPortador::Preferido
        } else {
            MotivoDoPortador::RedeComoAlternativa
        })
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
            portador_fixado: self.portador_fixado,
            motivo_do_portador: self.motivo_do_portador(),
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
