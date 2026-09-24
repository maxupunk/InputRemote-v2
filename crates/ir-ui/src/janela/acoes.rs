//! Os botões da janela ligados aos pedidos: configuração (política, borda, portador) e sessão
//! (encerrar, esquecer, recebidos, economia de energia, diagnóstico).
//!
//! Só tradução de clique em pedido. O que o clique muda chega de volta pelo aviso de estado.

use std::rc::Rc;

use ir_ipc::{Pedido, Resposta};
use slint::ComponentHandle;

use super::Contexto;
use crate::gerado::{Acoes, Dados, Janela};
use crate::ponte;

pub(super) fn ligar_configuracao(janela: &Janela, contexto: &Rc<Contexto>) {
    let acoes = janela.global::<Acoes>();

    let alvo = Rc::clone(contexto);
    acoes.on_definir_politica(move |indice| {
        alvo.enviar(Pedido::DefinirPolitica(ponte::politica_do_indice(indice)));
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

pub(super) fn ligar_sessao(janela: &Janela, contexto: &Rc<Contexto>) {
    let acoes = janela.global::<Acoes>();

    let alvo = Rc::clone(contexto);
    acoes.on_encerrar(move || alvo.enviar(Pedido::Encerrar));

    let alvo = Rc::clone(contexto);
    acoes.on_retomar(move || alvo.enviar(Pedido::Retomar));

    let alvo = Rc::clone(contexto);
    acoes.on_ctrl_alt_del(move || alvo.enviar(Pedido::CtrlAltDel));

    let alvo = Rc::clone(contexto);
    acoes.on_cancelar_copia(move || alvo.enviar(Pedido::CancelarCopia));

    let alvo = Rc::clone(contexto);
    acoes.on_abrir_recebidos(move || alvo.abrir_recebidos());

    let alvo = Rc::clone(contexto);
    acoes.on_travar_borda(move |travar| alvo.enviar(Pedido::TravarBorda(travar)));

    let alvo = Rc::clone(contexto);
    acoes.on_bloquear_juntos(move |juntos| alvo.enviar(Pedido::BloquearJuntos(juntos)));

    let alvo = Rc::clone(contexto);
    acoes.on_esquecer_par(move || {
        if let Some(maquina) = alvo.par_corrente() {
            alvo.enviar(Pedido::EsquecerPar { maquina });
        }
    });

    let alvo = Rc::clone(contexto);
    acoes.on_limpar_recebidos(move || {
        // O serviço responde "recebi" e apaga fora do compasso dele; o tamanho novo chega pelo
        // aviso de estado, e é ele que apaga o botão quando não sobra nada.
        alvo.enviar(Pedido::LimparRecebidos);
    });

    let alvo = Rc::clone(contexto);
    acoes.on_resolver_economia(move |no_par| {
        // A correção roda fora do compasso do serviço; o aviso some quando a verificação seguinte
        // chegar pelo aviso de estado — do outro computador, quando é a placa dele.
        alvo.enviar(Pedido::DesligarEconomiaDeEnergia { no_par });
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

impl Contexto {
    /// Abre a pasta de recebidos no gerenciador de arquivos do sistema.
    ///
    /// Quem abre é a janela, que roda como o usuário: o serviço não tem sessão gráfica.
    pub(super) fn abrir_recebidos(&self) {
        let pasta = self.pasta_de_recebidos.borrow().clone();
        if pasta.is_empty() {
            return;
        }
        let programa = if cfg!(windows) {
            "explorer"
        } else {
            "xdg-open"
        };
        if std::process::Command::new(programa)
            .arg(&pasta)
            .spawn()
            .is_err()
        {
            self.recado(Some(ir_ipc::Falha::SistemaRecusou));
        }
    }
}

impl Contexto {
    /// A conexão caiu sem ninguém pedir: um aviso fora da janela, porque quem estava usando o
    /// teclado do outro computador não está olhando para ela.
    ///
    /// Uma pausa, ou uma queda que a pessoa mesma provocou, não avisa: ela já sabe.
    pub(super) fn avisar_queda(&self, estado: &ir_ipc::Estado) {
        use ir_ipc::MotivoDaQueda as Q;
        if estado.pausa.is_some()
            || matches!(
                estado.ultima_queda,
                Some(Q::PedidoPeloUsuario | Q::TrocandoDeMeio)
            )
        {
            return;
        }
        let aberta = self
            .janela
            .upgrade()
            .is_some_and(|janela| janela.window().is_visible());
        if aberta {
            return;
        }
        let detalhe = estado.resumo();
        #[cfg(windows)]
        self.aviso.borrow_mut().mostrar(
            crate::gerado::CopiaUi {
                titulo: "Conexão perdida".into(),
                detalhe: detalhe.into(),
                progresso: 0.0,
                estado: crate::copia::PARADA,
                velocidade: slint::SharedString::default(),
                cancelavel: false,
                recebida: false,
            },
            true,
        );
        #[cfg(not(windows))]
        {
            let _ = std::process::Command::new("notify-send")
                .args(["--app-name=InputRemote", "Conexão perdida", &detalhe])
                .spawn();
        }
    }
}
