//! A ligação entre a janela e o serviço.
//!
//! Este módulo é a única parte da interface que sabe que existe um serviço. As telas leem
//! [`crate::gerado::Dados`] e disparam [`crate::gerado::Acoes`]; quem traduz isso em
//! [`ir_ipc::Pedido`] é aqui, e só aqui.
//!
//! Nenhuma decisão de produto acontece neste arquivo. Um `if` que decida *se* algo pode ser feito
//! é sinal de que a regra foi duplicada — o serviço já decide, e uma segunda cópia da regra na
//! interface é a cópia que vai ficar desatualizada.

mod acoes;
mod pareamento;

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

use ir_ipc::status::Estado;
use ir_ipc::vocabulario::Maquina;
use ir_ipc::{Autoridade, Aviso, Candidato, Falha, Pedido, Resposta};
use slint::{ComponentHandle, ModelRc, SharedString, Timer, TimerMode, VecModel, Weak};

use crate::gerado::{Dados, EtapaDoPareamento, Janela};
use crate::servico::{Servico, Situacao};
use crate::{ativacao, copia, ponte};

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
    /// O aviso de cópia no canto da tela, que aparece com a janela fechada.
    #[cfg(windows)]
    aviso: RefCell<crate::flutuante::Aviso>,
    /// A taxa da cópia em curso, medida entre avisos.
    velocimetro: RefCell<crate::historico::Velocimetro>,
    /// De qual cópia é a medida corrente.
    copia_medida: RefCell<Option<String>>,
    /// As últimas cópias e o tráfego da sessão.
    historico: RefCell<crate::historico::Historico>,
    /// Onde os recebidos ficam, pelo último estado, para o botão "Abrir a pasta".
    pasta_de_recebidos: RefCell<String>,
    /// Se o último estado tinha sessão de pé, para perceber a queda.
    conectado_antes: Cell<bool>,
    /// As últimas medianas de atraso, para o gráfico.
    atrasos: RefCell<crate::historico::Atrasos>,
}

impl Contexto {
    fn com_janela(&self, acao: impl FnOnce(&Janela)) {
        if let Some(janela) = self.janela.upgrade() {
            acao(&janela);
        }
    }

    fn aplicar(&self, estado: &Estado) {
        *self.par.borrow_mut() = estado.par.as_ref().map(|par| par.maquina);
        self.pasta_de_recebidos
            .replace(estado.pasta_de_recebidos.clone());
        let conectado = estado.enlace.conectado();
        if self.conectado_antes.replace(conectado) && !conectado {
            self.avisar_queda(estado);
        }
        let barras = {
            let mut atrasos = self.atrasos.borrow_mut();
            atrasos.anotar(estado.latencia.filter(|_| conectado).map(|m| m.mediana_ms));
            atrasos.barras()
        };
        let (recebidos, tem_o_que_limpar) = crate::historico::recebidos_ui(estado.recebidos_bytes);
        self.com_janela(|janela| {
            let dados = janela.global::<Dados>();
            dados.set_estado(ponte::estado_ui(estado));
            dados.set_recebidos(recebidos.clone().into());
            dados.set_recebidos_tem_o_que_limpar(tem_o_que_limpar);
            dados.set_atrasos(ModelRc::new(VecModel::from(barras.clone())));
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
            dados.set_recado_informativo(false);
        });
    }

    /// Um recado que não é falha: algo mudou sozinho, e a tela conta o porquê.
    fn informar(&self, frase: &'static str) {
        self.com_janela(|janela| {
            let dados = janela.global::<Dados>();
            dados.set_recado(frase.into());
            dados.set_recado_o_que_fazer(SharedString::new());
            dados.set_recado_informativo(true);
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
            Aviso::Transferencia(transferencia) => self.mostrar_copia(&transferencia),
            Aviso::PareamentoFalhou(falha) => self.falha_no_pareamento(falha),
            Aviso::Falhou(falha) => self.recado(Some(falha)),
            Aviso::PapelAjustado(papel) => self.informar(frase_do_papel_ajustado(papel)),
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

    /// Conta o que está acontecendo com uma cópia de arquivos.
    ///
    /// Na janela, num cartão que **fica** depois de terminar: a pergunta "aquilo copiou mesmo?"
    /// vem depois, quando a pessoa já está no outro computador. E, no Windows, também num aviso no
    /// canto da tela — porque quem copia está no Explorer, e não aqui.
    fn mostrar_copia(&self, transferencia: &ir_ipc::transferencia::Transferencia) {
        let velocidade = self.medir(transferencia);
        let copia = copia::copia_ui(transferencia, velocidade);
        self.com_janela(|janela| {
            let dados = janela.global::<Dados>();
            dados.set_copia(copia.clone());
            dados.set_tem_copia(true);
        });
        self.guardar_no_historico(transferencia);
        #[cfg(windows)]
        self.aviso
            .borrow_mut()
            .mostrar(copia, transferencia.terminou());
    }

    /// A taxa desta cópia, zerando o velocímetro quando começa outra.
    fn medir(&self, copia: &ir_ipc::transferencia::Transferencia) -> String {
        let mut velocimetro = self.velocimetro.borrow_mut();
        let mut anterior = self.copia_medida.borrow_mut();
        if anterior.as_deref() != Some(copia.nome.as_str()) {
            velocimetro.zerar();
            *anterior = Some(copia.nome.clone());
        }
        if copia.terminou() {
            // No fim não há taxa: há resultado. Mostrar a última medida ao lado de "Cópia
            // entregue" faria parecer que ainda está indo.
            velocimetro.zerar();
            return String::new();
        }
        velocimetro.medir(copia.bytes_feitos, std::time::Instant::now())
    }

    /// Guarda a cópia que terminou na lista das últimas, e atualiza o tráfego da sessão.
    fn guardar_no_historico(&self, copia: &ir_ipc::transferencia::Transferencia) {
        if !copia.terminou() {
            return;
        }
        let mut historico = self.historico.borrow_mut();
        historico.guardar(copia);
        let itens: Vec<crate::gerado::ItemDeCopia> = historico.itens().to_vec();
        let trafego = historico.trafego();
        self.com_janela(|janela| {
            let dados = janela.global::<Dados>();
            dados.set_copias_recentes(ModelRc::new(VecModel::from(itens.clone())));
            dados.set_trafego(trafego.clone().into());
        });
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

/// Diz ao ambiente gráfico quem são as janelas deste programa.
///
/// No Wayland o `app_id` é o que liga a janela ao `inputremote.desktop` — e é dele que vêm o ícone
/// na barra e o nome do aplicativo. Sem ele a janela nasce sem identidade: sem ícone, e o lançador
/// não a reconhece como o InputRemote já aberto. O nome é o do arquivo `.desktop` sem a extensão,
/// que é o que o GNOME procura.
///
/// **Depois** de a primeira janela existir, e não antes: a chamada precisa da plataforma gráfica já
/// escolhida, e antes disso ela falha com "no Slint platform was initialized" — que foi como este
/// defeito apareceu, no registro do próprio programa. O `app_id` é lido quando a janela é mostrada,
/// então aqui ainda é cedo o bastante.
fn identificar_as_janelas() {
    if let Err(erro) = slint::set_xdg_app_id("inputremote") {
        // No Windows e no macOS não há `app_id`, e a recusa é esperada; no Linux ela custa o ícone.
        if cfg!(target_os = "linux") {
            eprintln!("não consegui declarar o app_id da janela: {erro}");
        }
    }
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
    identificar_as_janelas();
    let situacao = servico.situacao();
    let contexto = Rc::new(Contexto {
        janela: janela.as_weak(),
        servico,
        candidatos: RefCell::new(Vec::new()),
        par: RefCell::new(None),
        situacao: Cell::new(situacao),
        #[cfg(windows)]
        aviso: RefCell::new(crate::flutuante::Aviso::novo()),
        velocimetro: RefCell::default(),
        copia_medida: RefCell::default(),
        historico: RefCell::default(),
        pasta_de_recebidos: RefCell::default(),
        conectado_antes: Cell::new(false),
        atrasos: RefCell::default(),
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
    acoes::ligar_configuracao(&janela, &contexto);
    pareamento::ligar(&janela, &contexto);
    acoes::ligar_sessao(&janela, &contexto);
    contexto.sincronizar();

    // A mesma batida recolhe os avisos e mantém a ligação: é consultando o serviço que a
    // interface percebe uma queda e tenta de novo, sem thread nem temporizador a mais.
    let cronometro = Timer::default();
    let batida = Rc::clone(&contexto);
    cronometro.start(TimerMode::Repeated, INTERVALO, move || {
        batida.escutar();
        batida.observar_conexao();
    });

    // Com a janela aberta, o estado a cada segundo: o atraso muda sem a fase mudar, e o serviço só
    // avisa quando a fase muda. Fechada, nada — ninguém está olhando o gráfico.
    let medidor = Timer::default();
    let olhar = Rc::clone(&contexto);
    medidor.start(
        TimerMode::Repeated,
        std::time::Duration::from_secs(1),
        move || {
            let aberta = olhar
                .janela
                .upgrade()
                .is_some_and(|janela| janela.window().is_visible());
            if aberta {
                olhar.sincronizar();
            }
        },
    );

    crate::bandeja::rodar(&janela, inicio, marca)
}

/// O que dizer quando este computador trocou de papel porque o outro escolheu o mesmo depois.
const fn frase_do_papel_ajustado(papel: ir_ipc::Papel) -> &'static str {
    match papel {
        ir_ipc::Papel::Cliente => {
            "O outro computador passou a ter o teclado, e este passou a ser controlado."
        }
        ir_ipc::Papel::Servidor => {
            "O outro computador passou a ser controlado, e o teclado daqui passou a controlar os dois."
        }
    }
}
