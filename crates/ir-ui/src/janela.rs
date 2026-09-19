//! A ligação entre a janela e o serviço.
//!
//! Este módulo é a única parte da interface que sabe que existe um serviço. As telas leem
//! [`crate::gerado::Dados`] e disparam [`crate::gerado::Acoes`]; quem traduz isso em
//! [`ir_ipc::Pedido`] é aqui, e só aqui.
//!
//! Nenhuma decisão de produto acontece neste arquivo. Um `if` que decida *se* algo pode ser feito
//! é sinal de que a regra foi duplicada — o serviço já decide, e uma segunda cópia da regra na
//! interface é a cópia que vai ficar desatualizada.

mod pareamento;

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

use ir_ipc::status::{Estado, Papel};
use ir_ipc::vocabulario::Maquina;
use ir_ipc::{Autoridade, Aviso, Candidato, Falha, Pedido, Resposta};
use slint::{ComponentHandle, ModelRc, SharedString, Timer, TimerMode, VecModel, Weak};

use crate::gerado::{Acoes, Dados, EtapaDoPareamento, Janela};
use crate::servico::{Servico, Situacao};
use crate::{ativacao, ponte};

/// De quanto em quanto tempo a interface recolhe avisos e confere a ligação com o serviço.
///
/// 200 ms é imperceptível para quem olha a janela e é barato para quem a serve. A interface não
/// está no caminho da entrada — teclado e mouse não passam por aqui —, então nada exige mais.
const INTERVALO: Duration = Duration::from_millis(200);

struct Contexto {
    janela: Weak<Janela>,
    servico: Rc<dyn Servico>,
    /// O que a última descoberta encontrou, para a tela poder escolher por posição.
    candidatos: RefCell<Vec<Candidato>>,
    /// O par corrente, que os pedidos precisam nomear.
    par: RefCell<Option<Maquina>>,
    /// A última situação da ligação que a janela mostrou: só se redesenha a faixa quando ela muda,
    /// e é a mudança para conectado que manda buscar o estado de novo.
    situacao: Cell<Situacao>,
}

impl Contexto {
    fn com_janela(&self, acao: impl FnOnce(&Janela)) {
        if let Some(janela) = self.janela.upgrade() {
            acao(&janela);
        }
    }

    fn aplicar(&self, estado: &Estado) {
        *self.par.borrow_mut() = estado.par.as_ref().map(|par| par.maquina);
        self.com_janela(|janela| {
            janela
                .global::<Dados>()
                .set_estado(ponte::estado_ui(estado));
        });
    }

    fn sincronizar(&self) {
        if let Resposta::Estado(estado) = self.servico.pedir(Pedido::Estado) {
            self.aplicar(&estado);
        }
    }

    /// Manda um pedido e cuida do resultado: recado na janela se falhar, estado novo se der.
    fn enviar(&self, pedido: Pedido) {
        match self.servico.pedir(pedido) {
            Resposta::Falha(falha) => self.recado(Some(falha)),
            _ => self.recado(None),
        }
        self.sincronizar();
    }

    fn recado(&self, falha: Option<Falha>) {
        // Toda falha carrega o que fazer a respeito, e as duas coisas aparecem juntas ou não
        // aparecem: "não deu" sem "e agora?" é a mensagem de erro que não ajuda ninguém.
        let (frase, acao) = falha.map_or_else(
            || (SharedString::new(), SharedString::new()),
            |falha| (falha.to_string().into(), falha.o_que_fazer().into()),
        );
        self.com_janela(|janela| {
            let dados = janela.global::<Dados>();
            dados.set_recado(frase.clone());
            dados.set_recado_o_que_fazer(acao.clone());
        });
    }

    fn escutar(&self) {
        for aviso in self.servico.avisos() {
            self.tratar(aviso);
        }
    }

    /// Confere a ligação com o serviço e atualiza a faixa se ela mudou.
    ///
    /// Ao voltar a conectar, pede o estado de novo: o que a tela mostrava é de antes da queda, e
    /// mostrar estado velho como se fosse atual é mentir para o usuário.
    fn observar_conexao(&self) {
        let agora = self.servico.situacao();
        if self.situacao.replace(agora) == agora {
            return;
        }
        self.mostrar_situacao(agora);
        if agora == Situacao::Conectado {
            self.sincronizar();
        }
    }

    /// Põe a situação da ligação na faixa do topo da janela.
    fn mostrar_situacao(&self, situacao: Situacao) {
        let orientacao = match situacao {
            Situacao::Desconectado(motivo) => {
                Some(ativacao::orientacao(motivo, ativacao::disponivel()))
            }
            Situacao::Conectado | Situacao::Simulado => None,
        };
        self.com_janela(|janela| {
            let dados = janela.global::<Dados>();
            dados.set_simulado(situacao == Situacao::Simulado);
            dados.set_desconectado(orientacao.is_some());
            let texto = |escolher: fn(&ativacao::Orientacao) -> &'static str| {
                orientacao.as_ref().map_or("", escolher).into()
            };
            dados.set_desconexao(texto(|o| o.frase));
            dados.set_desconexao_o_que_fazer(texto(|o| o.o_que_fazer));
            dados.set_ativacao_botao(texto(|o| o.botao));
            if orientacao.is_none() {
                // Ligou: o resultado de uma tentativa anterior não vale mais nada.
                dados.set_ativacao_resultado("".into());
            }
        });
    }

    fn tratar(&self, aviso: Aviso) {
        match aviso {
            Aviso::EstadoMudou(estado) => self.aplicar(&estado),
            Aviso::CandidatosEncontrados { candidatos } => self.mostrar_candidatos(candidatos),
            Aviso::CodigoDePareamento { digitos } => self.mostrar_codigo(digitos),
            Aviso::PareamentoConcluido { sucesso: true } => {
                self.etapa(EtapaDoPareamento::Concluido);
                self.sincronizar();
            }
            Aviso::PareamentoFalhou(falha) => self.falha_no_pareamento(falha),
            Aviso::PareamentoConcluido { sucesso: false } => {
                // Uma recusa que a própria janela pediu já está na tela com o motivo certo, e o
                // aviso que chega atrás dela não pode trocá-lo por um genérico. Fora isso, a
                // falha precisa de texto: um cartão de erro vazio é o mesmo que nenhuma reação.
                if self.etapa_atual() != Some(EtapaDoPareamento::Falhou) {
                    self.falha_no_pareamento(Falha::PareamentoInterrompido);
                }
                self.sincronizar();
            }
            // Reconciliação de teclas é ruído para o usuário: ela aparece no diagnóstico, onde
            // um número alto significa perda no meio do caminho, e em lugar nenhum mais.
            _ => {}
        }
    }

    fn etapa(&self, etapa: EtapaDoPareamento) {
        self.com_janela(|janela| janela.global::<Dados>().set_etapa(etapa));
    }

    /// A etapa que a tela de pareamento mostra agora.
    fn etapa_atual(&self) -> Option<EtapaDoPareamento> {
        self.janela
            .upgrade()
            .map(|janela| janela.global::<Dados>().get_etapa())
    }

    /// O par que os pedidos precisam nomear, ou um recado dizendo que não há par.
    fn par_corrente(&self) -> Option<Maquina> {
        let maquina = *self.par.borrow();
        if maquina.is_none() {
            self.recado(Some(Falha::ParDesconhecido));
        }
        maquina
    }

    fn falha_no_pareamento(&self, falha: Falha) {
        self.com_janela(|janela| {
            let dados = janela.global::<Dados>();
            dados.set_erro(falha.to_string().into());
            dados.set_erro_o_que_fazer(falha.o_que_fazer().into());
        });
        self.etapa(EtapaDoPareamento::Falhou);
    }
}

fn seis_vazios() -> ModelRc<SharedString> {
    ModelRc::new(VecModel::from(vec![SharedString::new(); 6]))
}

/// Abre a janela e roda até ela fechar.
///
/// # Errors
///
/// Repassa a falha de [`slint::PlatformError`] quando não há como criar ou rodar a janela — sem
/// backend gráfico, ou sem servidor de janelas no Linux.
pub fn abrir(
    servico: Rc<dyn Servico>,
    inicio: crate::bandeja::Inicio,
    marca: crate::bandeja::Marca,
) -> Result<(), slint::PlatformError> {
    let janela = Janela::new()?;
    let situacao = servico.situacao();
    let contexto = Rc::new(Contexto {
        janela: janela.as_weak(),
        servico,
        candidatos: RefCell::new(Vec::new()),
        par: RefCell::new(None),
        situacao: Cell::new(situacao),
    });

    {
        let dados = janela.global::<Dados>();
        dados.set_elevado(contexto.servico.autoridade() >= Autoridade::Elevado);
        dados.set_digitos(seis_vazios());
    }
    contexto.mostrar_situacao(situacao);

    let depois = Rc::clone(&contexto);
    let _ativacao = ativacao::ligar(&janela, move || {
        depois.servico.tentar_agora();
        depois.observar_conexao();
    });
    ligar_configuracao(&janela, &contexto);
    pareamento::ligar(&janela, &contexto);
    ligar_sessao(&janela, &contexto);
    contexto.sincronizar();

    // A mesma batida recolhe os avisos e mantém a ligação: é consultando o serviço que a
    // interface percebe uma queda e tenta de novo, sem thread nem temporizador a mais.
    let cronometro = Timer::default();
    let batida = Rc::clone(&contexto);
    cronometro.start(TimerMode::Repeated, INTERVALO, move || {
        batida.escutar();
        batida.observar_conexao();
    });

    crate::bandeja::rodar(&janela, inicio, marca)
}

fn ligar_configuracao(janela: &Janela, contexto: &Rc<Contexto>) {
    let acoes = janela.global::<Acoes>();

    let alvo = Rc::clone(contexto);
    acoes.on_definir_papel(move |servidor| {
        let papel = if servidor {
            Papel::Servidor
        } else {
            Papel::Cliente
        };
        alvo.enviar(Pedido::DefinirPapel(papel));
    });

    let alvo = Rc::clone(contexto);
    acoes.on_definir_borda(move |indice| {
        alvo.enviar(Pedido::DefinirBorda(ponte::borda_do_indice(indice)));
    });

    let alvo = Rc::clone(contexto);
    acoes.on_fixar_portador(move |indice| {
        alvo.enviar(Pedido::FixarPortador(ponte::portador_do_indice(indice)));
    });

    let alvo = Rc::clone(contexto);
    acoes.on_permitir_bloqueio(move |permitir| {
        if let Some(maquina) = alvo.par_corrente() {
            alvo.enviar(Pedido::PermitirTelaDeBloqueio { maquina, permitir });
        }
    });
}

fn ligar_sessao(janela: &Janela, contexto: &Rc<Contexto>) {
    let acoes = janela.global::<Acoes>();

    let alvo = Rc::clone(contexto);
    acoes.on_encerrar(move || alvo.enviar(Pedido::Encerrar));

    let alvo = Rc::clone(contexto);
    acoes.on_esquecer_par(move || {
        if let Some(maquina) = alvo.par_corrente() {
            alvo.enviar(Pedido::EsquecerPar { maquina });
        }
    });

    let alvo = Rc::clone(contexto);
    acoes.on_mostrar_diagnostico(move || {
        if let Resposta::Diagnostico(relatorio) = alvo.servico.pedir(Pedido::Diagnostico) {
            alvo.com_janela(|janela| {
                janela
                    .global::<Dados>()
                    .set_diagnostico(relatorio.clone().into());
            });
        }
    });

    let alvo = Rc::clone(contexto);
    acoes.on_descartar_recado(move || alvo.recado(None));
}
