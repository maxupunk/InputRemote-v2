//! Um serviço de mentira, para a interface poder ser olhada e medida hoje.
//!
//! Não é maquete de tela: é uma máquina de estados de verdade, que passa pelas mesmas transições
//! que o serviço vai passar — descobrir, esperar, mostrar o código, conectar, medir atraso — e que
//! responde às mesmas mensagens. Isso torna a interface testável de ponta a ponta antes de existir
//! transporte, e obriga os dois lados a concordarem com o mesmo contrato.
//!
//! O relógio avança em [`Servico::avisos`], que a janela consulta em intervalo fixo. Um passo é um
//! passo, sempre — então os testes conduzem o tempo sem dormir, e o resultado é determinístico.
//!
//! Só entra quando pedido de propósito (`--simulado`), e a janela avisa na cara do usuário quando
//! ele está em uso ([`Servico::situacao`]). Uma interface que finge estar funcionando é pior que
//! uma que não abre.

use std::cell::RefCell;

use ir_ipc::status::{
    Estado, Latencia, LinkState, MotivoDaQueda, MotivoDoPortador, Papel, ParConhecido,
};
use ir_ipc::vocabulario::{Clipboard, Maquina, Nivel, Nome, Portador, Recursos};
use ir_ipc::{Autoridade, Aviso, Candidato, Falha, Pedido, Resposta};

use crate::servico::{Servico, Situacao};

/// Os seis dígitos que a demonstração mostra.
const DIGITOS: [u8; 6] = [4, 1, 9, 0, 7, 3];

/// Quantos passos cada espera simulada leva. A janela consulta a cada 200 ms.
const PASSOS_DESCOBERTA: u32 = 6;
const PASSOS_CODIGO: u32 = 5;
const PASSOS_CONEXAO: u32 = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Passo {
    Descobriu,
    MostrouCodigo,
    Pareou,
    Conectou,
}

#[derive(Debug)]
struct Interno {
    estado: Estado,
    tique: u32,
    agenda: Vec<(u32, Passo)>,
    fila: Vec<Aviso>,
    candidatos: Vec<Candidato>,
    escolhido: Option<Candidato>,
}

/// Um serviço simulado, com uma máquina de estados que se comporta como a de verdade.
#[derive(Debug)]
pub struct ServicoSimulado {
    interno: RefCell<Interno>,
}

impl Default for ServicoSimulado {
    fn default() -> Self {
        Self::new()
    }
}

impl ServicoSimulado {
    /// Uma máquina recém-instalada, com o agente pronto e nada pareado.
    ///
    /// O agente pronto é deliberado: um simulado que já começa com impedimento mostraria a tela de
    /// erro, e o que se quer avaliar é a tela normal.
    #[must_use]
    pub fn new() -> Self {
        let mut estado =
            Estado::recem_instalado(Maquina([0x5A; 16]), Nome::coagido("esta-bancada"));
        estado.agente_pronto = true;
        estado.nivel_privilegiado = Nivel::TelaDeBloqueio;

        Self {
            interno: RefCell::new(Interno {
                estado,
                tique: 0,
                agenda: Vec::new(),
                fila: Vec::new(),
                candidatos: Vec::new(),
                escolhido: None,
            }),
        }
    }
}

impl Interno {
    fn agendar(&mut self, daqui: u32, passo: Passo) {
        let quando = self.tique.saturating_add(daqui);
        self.agenda.push((quando, passo));
    }

    fn anunciar_estado(&mut self) {
        self.fila.push(Aviso::EstadoMudou(self.estado.clone()));
    }

    fn avancar(&mut self) {
        self.tique = self.tique.saturating_add(1);
        let vencidos: Vec<Passo> = self
            .agenda
            .iter()
            .filter(|(quando, _)| *quando <= self.tique)
            .map(|(_, passo)| *passo)
            .collect();
        self.agenda.retain(|(quando, _)| *quando > self.tique);
        for passo in vencidos {
            self.executar(passo);
        }
    }

    fn executar(&mut self, passo: Passo) {
        match passo {
            Passo::Descobriu => {
                self.candidatos = candidatos_de_demonstracao();
                self.fila.push(Aviso::CandidatosEncontrados {
                    candidatos: self.candidatos.clone(),
                });
            }
            Passo::MostrouCodigo => {
                self.fila
                    .push(Aviso::CodigoDePareamento { digitos: DIGITOS });
            }
            Passo::Pareou => {
                self.parear();
                self.fila.push(Aviso::PareamentoConcluido { sucesso: true });
                self.anunciar_estado();
                self.agendar(PASSOS_CONEXAO, Passo::Conectou);
            }
            Passo::Conectou => {
                self.conectar();
                self.anunciar_estado();
            }
        }
    }

    fn parear(&mut self) {
        let Some(candidato) = self.escolhido.clone() else {
            return;
        };
        self.estado.par = Some(ParConhecido {
            maquina: Maquina([0xC3; 16]),
            nome: Nome::coagido(&candidato.rotulo),
            recursos: Recursos {
                clipboard: Clipboard::SO_TEXTO,
                transferencia: true,
                nivel: Nivel::TelaDeBloqueio,
                atencao_segura: false,
            },
            conectado: false,
        });
        self.estado.enlace = LinkState::Conectando;
        self.estado.ultima_queda = None;
    }

    fn conectar(&mut self) {
        let portador = self.estado.portador_fixado.unwrap_or(Portador::Bluetooth);
        self.estado.enlace = LinkState::Pronto;
        self.estado.portador = Some(portador);
        self.estado.motivo_do_portador = Some(if self.estado.portador_fixado.is_some() {
            MotivoDoPortador::FixadoPeloUsuario
        } else {
            MotivoDoPortador::Preferido
        });
        self.estado.latencia = Some(latencia_de(portador));
        if let Some(par) = self.estado.par.as_mut() {
            par.conectado = true;
        }
    }

    fn encerrar(&mut self) {
        self.estado.enlace = LinkState::Desconectado;
        self.estado.portador = None;
        self.estado.motivo_do_portador = None;
        self.estado.latencia = None;
        self.estado.ultima_queda = Some(MotivoDaQueda::PedidoPeloUsuario);
        if let Some(par) = self.estado.par.as_mut() {
            par.conectado = false;
        }
        self.agenda.clear();
    }

    fn mudar(&mut self, pedido: Pedido) -> Resposta {
        match pedido {
            Pedido::DefinirPapel(papel) => {
                self.estado.papel = papel;
            }
            Pedido::DefinirBorda(borda) => {
                self.estado.borda_do_par = borda;
            }
            Pedido::FixarPortador(portador) => {
                self.estado.portador_fixado = portador;
                if self.estado.enlace.conectado() {
                    self.conectar();
                }
            }
            Pedido::PermitirTelaDeBloqueio { permitir, .. } => {
                self.estado.bloqueio_permitido = permitir;
            }
            outro => return self.parear_conforme(outro),
        }
        self.anunciar_estado();
        Resposta::Feito
    }

    fn parear_conforme(&mut self, pedido: Pedido) -> Resposta {
        match pedido {
            Pedido::Procurar => {
                self.candidatos.clear();
                self.agendar(PASSOS_DESCOBERTA, Passo::Descobriu);
                Resposta::Feito
            }
            Pedido::IniciarPareamento { candidato } => self.iniciar_pareamento(&candidato),
            Pedido::ConfirmarPareamento { conferiu } => {
                if !conferiu {
                    // Códigos diferentes é sinal de alguém no meio. Não se tenta de novo: se
                    // desiste desta rede.
                    self.escolhido = None;
                    return Resposta::Falha(Falha::CodigosDiferentes);
                }
                self.agendar(1, Passo::Pareou);
                Resposta::Feito
            }
            Pedido::EsquecerPar { .. } => {
                self.encerrar();
                self.estado.par = None;
                self.estado.ultima_queda = None;
                self.escolhido = None;
                self.anunciar_estado();
                Resposta::Feito
            }
            _ => Resposta::Falha(Falha::ForaDeContexto),
        }
    }

    fn iniciar_pareamento(&mut self, endereco: &str) -> Resposta {
        let achado = self
            .candidatos
            .iter()
            .find(|item| item.endereco == endereco)
            .cloned();
        let Some(candidato) = achado else {
            return Resposta::Falha(Falha::ParDesconhecido);
        };
        self.escolhido = Some(candidato);
        self.agendar(PASSOS_CODIGO, Passo::MostrouCodigo);
        Resposta::Feito
    }
}

impl Servico for ServicoSimulado {
    fn pedir(&self, pedido: Pedido) -> Resposta {
        let mut interno = self.interno.borrow_mut();
        match pedido {
            Pedido::Estado | Pedido::Acompanhar => Resposta::Estado(interno.estado.clone()),
            Pedido::Diagnostico => Resposta::Diagnostico(diagnostico(&interno.estado)),
            Pedido::Encerrar => {
                interno.encerrar();
                interno.anunciar_estado();
                Resposta::Feito
            }
            outro => interno.mudar(outro),
        }
    }

    fn avisos(&self) -> Vec<Aviso> {
        let mut interno = self.interno.borrow_mut();
        interno.avancar();
        core::mem::take(&mut interno.fila)
    }

    fn autoridade(&self) -> Autoridade {
        // O simulado se declara elevado para que o fluxo de pareamento possa ser percorrido inteiro
        // na demonstração. Quem decide de verdade é o serviço, que verifica o token do cliente e
        // não acredita no que o cliente diz sobre si.
        Autoridade::Elevado
    }

    fn situacao(&self) -> Situacao {
        Situacao::Simulado
    }
}

fn candidatos_de_demonstracao() -> Vec<Candidato> {
    vec![
        Candidato {
            rotulo: "bancada-linux".to_owned(),
            endereco: "192.168.0.24:7311".to_owned(),
            portador: Portador::RedeLocal,
        },
        Candidato {
            rotulo: "notebook".to_owned(),
            endereco: "A4:C1:38:0B:5E:22".to_owned(),
            portador: Portador::Bluetooth,
        },
    ]
}

/// Números plausíveis, e não bonitos: são as metas de `docs/01-visao-e-escopo.md` §6 com folga
/// pequena. Um simulado que mostra 1 ms treina o olho errado.
fn latencia_de(portador: Portador) -> Latencia {
    match portador {
        Portador::Bluetooth => Latencia {
            mediana_ms: 14,
            p99_ms: 38,
            amostras: 512,
        },
        Portador::RedeLocal | Portador::RedeDeArquivos => Latencia {
            mediana_ms: 5,
            p99_ms: 19,
            amostras: 512,
        },
    }
}

/// O relatório de diagnóstico.
///
/// Lista fechada, montada campo por campo. **Nunca** despeja estado inteiro: o produto vê senhas, e
/// um relatório que o usuário cola num relato público não pode conter nada do que foi digitado
/// ([04, §7](../../../docs/04-seguranca.md)).
fn diagnostico(estado: &Estado) -> String {
    let campos: [(&str, String); 12] = [
        ("maquina", estado.este_nome.como_texto().to_owned()),
        ("impressao", estado.esta_maquina.impressao()),
        ("papel", papel_de(estado).to_owned()),
        ("enlace", estado.enlace.frase().to_owned()),
        ("borda", estado.borda_do_par.nome().to_owned()),
        (
            "portador",
            estado
                .portador
                .map_or("nenhum", Portador::nome_tecnico)
                .to_owned(),
        ),
        (
            "motivo",
            estado
                .motivo_do_portador
                .map_or("-", MotivoDoPortador::frase)
                .to_owned(),
        ),
        ("atraso", atraso_de(estado)),
        ("nivel", nivel_de(estado)),
        ("agente pronto", estado.agente_pronto.to_string()),
        ("bloqueio permitido", estado.bloqueio_permitido.to_string()),
        (
            "ultima queda",
            estado
                .ultima_queda
                .map_or("nenhuma", MotivoDaQueda::frase)
                .to_owned(),
        ),
    ];

    let mut linhas = Vec::with_capacity(campos.len() + 1);
    linhas.push("InputRemote - diagnostico (servico simulado)".to_owned());
    for (rotulo, valor) in campos {
        linhas.push(format!("{rotulo}: {valor}"));
    }
    linhas.join("\n")
}

fn papel_de(estado: &Estado) -> &'static str {
    if estado.papel == Papel::Servidor {
        "servidor"
    } else {
        "cliente"
    }
}

fn atraso_de(estado: &Estado) -> String {
    estado.latencia.map_or_else(
        || "sem amostras".to_owned(),
        |medida| {
            format!(
                "mediana {} ms, p99 {} ms, {} amostras",
                medida.mediana_ms, medida.p99_ms, medida.amostras
            )
        },
    )
}

/// O nível de capacidade por extenso, para o relatório valer sozinho.
///
/// "N2" não diz nada a quem lê um relato colado num registro de problema.
fn nivel_de(estado: &Estado) -> String {
    let nivel = estado.nivel_privilegiado;
    format!("{} ({})", nivel.rotulo(), nivel.explicacao())
}
