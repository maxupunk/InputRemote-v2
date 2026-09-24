//! A face de IPC do ator: traduzir pedidos da interface em ação, e o estado interno no
//! [`Estado`] publicado.
//!
//! Fica separado do laço central ([`super`]) por tamanho e por assunto: aqui é só a fronteira
//! com a interface — nenhuma decisão de sessão acontece neste arquivo, só a tradução entre o
//! vocabulário de dentro e o de fora. As `impl` são do mesmo [`Daemon`]; um submódulo enxerga
//! os campos privados do pai, então nada precisou virar público para isto morar aqui.

use ir_ipc::{Aviso, Candidato, Falha, Pedido, Portador, Resposta};
use tracing::error;

use super::Daemon;
use crate::config::Config;
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
                self.abrir_para_pareamento();
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
            Pedido::Encerrar => self.pausar(),
            Pedido::Retomar => self.retomar(),
            Pedido::CtrlAltDel => {
                self.drive(ir_session::Input::SecureAttention);
                Resposta::Feito
            }
            Pedido::TravarBorda(travar) => {
                self.borda_travada = travar;
                self.drive(ir_session::Input::LockEdge(travar));
                let _ = self.avisos.send(Aviso::EstadoMudou(self.estado()));
                Resposta::Feito
            }
            Pedido::BloquearJuntos(juntos) => {
                let mut nova = self.config.clone();
                nova.bloquear_juntos = juntos;
                let resposta = self.persistir(nova);
                let _ = self.avisos.send(Aviso::EstadoMudou(self.estado()));
                resposta
            }
            Pedido::EsquecerPar { .. } => self.esquecer_par(),
            Pedido::FixarPortador(portador) => self.fixar_portador(portador),
            // As duas trocas valem na hora (`super::papel`): a de papel refaz a sessão, e a de borda
            // só a ajusta — e só no servidor, que é quem decide a borda.
            Pedido::DefinirBorda(borda) => self.trocar_borda(borda.no_protocolo()),
            Pedido::DefinirPolitica(p) => self.definir_politica(ir_painel::policy_de(p)),
            Pedido::Diagnostico => Resposta::Diagnostico(self.diagnostico()),
            // Arquivos e clipboard são o outro assunto desta conexão, e ficam juntos.
            outro => self.tratar_conteudo(outro, leitor),
        }
    }

    /// Fixa um portador, ou volta à escolha automática — e grava, para valer depois de reiniciar.
    ///
    /// Guardado dos dois lados: a sessão precisa dele para desligar a degradação, e a interface
    /// precisa vê-lo de volta para dizer "fixado nas preferências" em vez de inventar um motivo.
    pub(super) fn fixar_portador(&mut self, portador: Option<Portador>) -> Resposta {
        let mut nova = self.config.clone();
        nova.portador_fixado =
            portador.map(|portador| ir_painel::texto_do_portador(portador).to_owned());
        if let Resposta::Falha(falha) = self.persistir(nova) {
            return Resposta::Falha(falha);
        }
        self.portador_fixado = portador;
        self.session
            .pin_carrier(portador.map(Portador::no_protocolo), &mut self.out);
        self.apply_commands();
        let _ = self.avisos.send(Aviso::EstadoMudou(self.estado()));
        Resposta::Feito
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
            Pedido::DesligarEconomiaDeEnergia { no_par } => self.desligar_economia(no_par),
            Pedido::PermitirTelaDeBloqueio { permitir, .. } => {
                self.permitir_tela_de_bloqueio(permitir)
            }
            Pedido::CancelarCopia => {
                if self.arquivos.cancelar() {
                    Resposta::Feito
                } else {
                    Resposta::Falha(Falha::ForaDeContexto)
                }
            }
            // O curinga cobre variantes futuras do contrato ainda não tratadas aqui.
            _ => Resposta::Falha(Falha::ForaDeContexto),
        }
    }

    /// Leva o texto copiado ao par, pelo canal 4 da sessão.
    fn oferecer_texto(&mut self, texto: ir_ipc::TextoDoClipboard) -> Resposta {
        if !self.session.phase().is_established() {
            return Resposta::Falha(Falha::SemConexao);
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
            Resposta::Falha(Falha::SemConexao)
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
            return Resposta::Falha(Falha::EnderecoInvalido);
        };
        self.abrir_para_pareamento();
        if self.transporte(alvo.portador()).is_none() {
            // Pediram para parear por um portador que não está aberto nesta máquina — sem rádio,
            // por exemplo. Dizer isso é melhor que ficar em silêncio esperando um código.
            return Resposta::Falha(Falha::SemBluetooth);
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

    /// Liga ou desliga a digitação do par na tela de bloqueio e nos pedidos de permissão daqui.
    ///
    /// Gravado por par, e o agente fica sabendo na hora: é ele quem recusa injetar no desktop
    /// protegido quando a permissão não existe ([04, §6](../../../docs/04-seguranca.md)).
    fn permitir_tela_de_bloqueio(&mut self, permitir: bool) -> Resposta {
        if self.config.peers.is_empty() {
            return Resposta::Falha(Falha::ParDesconhecido);
        }
        let mut nova = self.config.clone();
        if let Some(par) = nova.peers.first_mut() {
            par.tela_de_bloqueio = permitir;
        }
        #[cfg(windows)]
        let nova = self.politica_de_atencao(nova, permitir);
        let resposta = self.persistir(nova);
        if resposta == Resposta::Feito {
            tracing::info!(permitir, "digitação do par na tela de bloqueio");
            self.contar_ao_agente_a_permissao();
            let _ = self.avisos.send(Aviso::EstadoMudou(self.estado()));
        }
        resposta
    }

    /// Grava a configuração nova e **só então** a adota.
    ///
    /// Um ponto só para toda ação que grava. A ordem é o que importa: se a gravação falha, nem o
    /// arquivo nem a memória mudam, e os dois nunca divergem — antes a memória mudava primeiro, e
    /// uma gravação que falhasse deixava o serviço usando um valor que o próximo reinício perderia.
    pub(super) fn persistir(&mut self, nova: Config) -> Resposta {
        match self.gravador.gravar_e_esperar(&nova) {
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
}
