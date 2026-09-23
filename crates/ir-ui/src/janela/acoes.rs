//! Os botões da janela ligados aos pedidos: configuração (papel, borda, portador) e sessão
//! (encerrar, esquecer, recebidos, economia de energia, diagnóstico).
//!
//! Só tradução de clique em pedido. O que o clique muda chega de volta pelo aviso de estado.

use std::rc::Rc;

use ir_ipc::status::Papel;
use ir_ipc::{Pedido, Resposta};
use slint::ComponentHandle;

use super::Contexto;
use crate::gerado::{Acoes, Dados, Janela};
use crate::ponte;

pub(super) fn ligar_configuracao(janela: &Janela, contexto: &Rc<Contexto>) {
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

pub(super) fn ligar_sessao(janela: &Janela, contexto: &Rc<Contexto>) {
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
