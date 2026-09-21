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
        let PedidoRecebido {
            pedido,
            leitor,
            responder,
        } = recebido;
        let resposta = self.tratar(pedido, leitor);
        let _ = responder.send(resposta);
    }

    /// A ação de cada pedido. A autoridade é conferida no transporte, não aqui.
    pub(super) fn tratar(&mut self, pedido: Pedido, leitor: ir_transferencia::Leitor) -> Resposta {
        match pedido {
            Pedido::Estado => Resposta::Estado(self.estado()),
            Pedido::Acompanhar => {
                self.recontar_codigo_pendente();
                Resposta::Feito
            }
            // Quem conta o ajudante é a conexão dele (`ipc::controle`); aqui não há o que fazer.
            Pedido::AcompanharClipboard => Resposta::Feito,
            Pedido::LimparRecebidos => {
                self.arquivos.limpar_recebidos(&self.avisos, self.estado());
                Resposta::Feito
            }
            Pedido::Procurar => {
                self.procurar();
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
            // Arquivos e clipboard são o outro assunto desta conexão, e ficam juntos.
            outro => self.tratar_conteudo(outro, leitor),
        }
    }

    /// Os pedidos sobre o que atravessa: arquivos e clipboard.
    fn tratar_conteudo(&mut self, pedido: Pedido, leitor: ir_transferencia::Leitor) -> Resposta {
        match pedido {
            Pedido::EnviarArquivos { caminhos } => self.enviar_arquivos(caminhos, leitor),
            // O mesmo gatilho da travessia, à mão. Quem lê o clipboard é o ajudante da sessão.
            Pedido::SincronizarClipboard => {
                let _ = self.avisos.send(Aviso::LerClipboard);
                Resposta::Feito
            }
            // O texto vem de quem pediu, e não de um caminho que o serviço leria: não há o que
            // conferir de permissão, só se há par para receber.
            Pedido::OferecerTexto(texto) => self.oferecer_texto(texto),
            // A tela de bloqueio é N2: depende do agente no desktop seguro, que ainda não entra.
            // O curinga cobre também variantes futuras do contrato ainda não tratadas aqui.
            _ => Resposta::Falha(Falha::ForaDeContexto),
        }
    }

    /// Leva o texto copiado ao par, pelo canal 4 da sessão.
    fn oferecer_texto(&mut self, texto: ir_ipc::TextoDoClipboard) -> Resposta {
        if !self.session.phase().is_established() {
            return Resposta::Falha(Falha::ForaDeContexto);
        }
        // Os dois tipos têm o mesmo limite, o do canal 4; a conversão não recusa nada.
        let Some(texto) = ir_session::ClipText::new(texto.em_string()) else {
            return Resposta::Falha(Falha::ForaDeContexto);
        };
        self.drive(ir_session::Input::ClipboardText(texto));
        Resposta::Feito
    }

    /// Encaminha um pedido de envio para a tarefa de transferência.
    ///
    /// O ator **não espera** a transferência: ela pode levar minutos, e ele gira a cada 5 ms. A
    /// resposta é "recebi o pedido", e o que acontece depois chega por
    /// [`Aviso::Transferencia`](ir_ipc::Aviso::Transferencia).
    ///
    /// Quem não pode ler nada é recusado aqui, antes de a tarefa de transferência tocar o disco: é o
    /// serviço do Windows como SYSTEM, que ainda não sabe quem está do outro lado do *pipe*.
    fn enviar_arquivos(&self, caminhos: Vec<String>, leitor: ir_transferencia::Leitor) -> Resposta {
        if leitor == ir_transferencia::Leitor::Desconhecido {
            return Resposta::Falha(Falha::SemPermissao);
        }
        let caminhos = caminhos.into_iter().map(std::path::PathBuf::from).collect();
        if self.arquivos.enviar(caminhos, leitor) {
            Resposta::Feito
        } else {
            Resposta::Falha(Falha::ForaDeContexto)
        }
    }

    /// Procura quem está por perto — rede e rádio — e conta à janela quando terminar.
    ///
    /// A busca leva segundos e roda numa tarefa própria: o ator não espera, e o resultado vai direto
    /// aos avisos. O endereço da configuração (ou o do último par) entra como candidato marcado.
    fn procurar(&self) {
        let configurado = self
            .config
            .peer_addr
            .as_deref()
            .and_then(Endereco::ler)
            .or_else(|| {
                self.config
                    .peers
                    .first()
                    .and_then(|par| par.addr.as_deref())
                    .and_then(Endereco::ler)
            });
        let busca = self.descoberta.procurar(configurado);
        let avisos = self.avisos.clone();
        tokio::spawn(async move {
            let candidatos = busca
                .await
                .into_iter()
                .map(|encontrado| Candidato {
                    rotulo: encontrado.rotulo,
                    endereco: encontrado.endereco.to_string(),
                    portador: Portador::from(encontrado.endereco.portador()),
                })
                .collect();
            let _ = avisos.send(Aviso::CandidatosEncontrados { candidatos });
        });
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
        self.discagem_comecou(alvo.portador());
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
             rádio Bluetooth: {}\nportador em uso: {}\najudantes de clipboard ligados: {}",
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
            self.ajudantes.ligados(),
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
            recebidos_bytes: self.arquivos.recebidos().espaco(),
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
