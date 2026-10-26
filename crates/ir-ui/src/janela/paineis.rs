//! A tela do painel no celular ligada aos pedidos.
//!
//! Só tradução, como a das pastas: o clique vira [`ComandoDePainel`], e o aviso vira etapa, dígitos
//! e lista. A tela escolhe por posição, e é aqui que a posição vira o aparelho — o identificador não
//! sai do Rust.

use std::cell::RefCell;
use std::rc::Rc;

use ir_ipc::{
    AvisoDoPainel, ComandoDePainel, Pedido, Resposta, ResumoDoPainel, resumo_dos_paineis,
};
use slint::{ComponentHandle, ModelRc, SharedString, VecModel};

use super::Contexto;
use crate::gerado::{Acoes, Dados, EtapaDoPainel, Janela, PainelUi};

/// A lista como a tela a mostra, na mesma ordem.
pub(super) type ListaDePaineis = RefCell<Vec<ResumoDoPainel>>;

pub(super) fn ligar(janela: &Janela, contexto: &Rc<Contexto>) {
    let acoes = janela.global::<Acoes>();

    let alvo = Rc::clone(contexto);
    acoes.on_parear_painel(move || {
        alvo.mostrar_codigo_do_painel(None);
        alvo.comando_de_painel(ComandoDePainel::Parear, EtapaDoPainel::Esperando);
    });

    let alvo = Rc::clone(contexto);
    acoes.on_confirmar_painel(move |conferiu| {
        // Recusado aqui, acabou aqui: o aviso que chega depois não deve trocar a tela de novo.
        let depois = if conferiu {
            EtapaDoPainel::Esperando
        } else {
            EtapaDoPainel::Parado
        };
        alvo.comando_de_painel(ComandoDePainel::Confirmar(conferiu), depois);
    });

    let alvo = Rc::clone(contexto);
    acoes.on_cancelar_painel(move || {
        alvo.comando_de_painel(ComandoDePainel::Cancelar, EtapaDoPainel::Parado);
    });

    let alvo = Rc::clone(contexto);
    acoes.on_aceitar_pedidos_de_paineis(move |aceita| {
        alvo.pedir_sobre_paineis(ComandoDePainel::AceitarPedidos(aceita));
    });

    let alvo = Rc::clone(contexto);
    acoes.on_esquecer_painel(move |posicao| {
        let id = usize::try_from(posicao)
            .ok()
            .and_then(|i| alvo.paineis.borrow().get(i).map(|p| p.id.clone()));
        if let Some(id) = id {
            let etapa = alvo.etapa_do_painel();
            alvo.comando_de_painel(ComandoDePainel::Esquecer(id), etapa);
        }
    });
}

impl Contexto {
    /// Manda um pedido sobre os painéis e põe a tela na etapa seguinte, se deu certo.
    fn comando_de_painel(&self, comando: ComandoDePainel, depois: EtapaDoPainel) {
        if let Resposta::Falha(falha) = self.servico.pedir(Pedido::Painel(comando)) {
            self.recado(Some(falha));
            return;
        }
        self.recado(None);
        self.com_janela(|janela| janela.global::<Dados>().set_painel_etapa(depois));
    }

    /// Um pedido sobre os painéis que não muda a etapa da tela: a resposta chega como aviso.
    fn pedir_sobre_paineis(&self, comando: ComandoDePainel) {
        if let Resposta::Falha(falha) = self.servico.pedir(Pedido::Painel(comando)) {
            self.recado(Some(falha));
        }
    }

    /// Pede a lista ao serviço — ao abrir e ao reconectar —, e se os pedidos de aparelhos novos
    /// estão aceitos.
    pub(super) fn buscar_paineis(&self) {
        if let Resposta::Paineis(lista) = self.servico.pedir(Pedido::Paineis) {
            self.mostrar_paineis(lista);
        }
        // Só para o interruptor da tela: um serviço de antes desta versão não o conhece, e a falha
        // dele não é recado para ninguém.
        let _ = self
            .servico
            .pedir(Pedido::Painel(ComandoDePainel::ComoEstaoOsPedidos));
    }

    /// O que o serviço contou sobre os painéis.
    pub(super) fn aviso_do_painel(&self, aviso: AvisoDoPainel) {
        match aviso {
            AvisoDoPainel::Codigo(digitos) => {
                self.mostrar_codigo_do_painel(Some(digitos));
                self.com_janela(|janela| {
                    janela
                        .global::<Dados>()
                        .set_painel_etapa(EtapaDoPainel::Comparando);
                    // O aparelho chegou — e, com o pedido partindo dele, ninguém aqui clicou em
                    // nada: a comparação vem para a frente, mesmo com a janela na bandeja.
                    janela.invoke_mostrar_paineis();
                    let janela = janela.window();
                    janela.set_minimized(false);
                    let _ = janela.show();
                });
                self.recados
                    .borrow_mut()
                    .avulso(&ir_recado::Recado::pedido_de_painel(digitos));
            }
            AvisoDoPainel::AceitaPedidos(aceita) => self.com_janela(|janela| {
                janela.global::<Dados>().set_painel_aceita_pedidos(aceita);
            }),
            AvisoDoPainel::Concluido(sucesso) => {
                let etapa = self.etapa_do_painel();
                // Só um pareamento em curso termina; um "não" que esta tela já mostrou fica.
                if matches!(etapa, EtapaDoPainel::Esperando | EtapaDoPainel::Comparando) {
                    let fim = if sucesso {
                        EtapaDoPainel::Concluido
                    } else {
                        EtapaDoPainel::Falhou
                    };
                    self.com_janela(|janela| janela.global::<Dados>().set_painel_etapa(fim));
                }
            }
            AvisoDoPainel::Lista(lista) => self.mostrar_paineis(lista),
        }
    }

    fn mostrar_paineis(&self, lista: Vec<ResumoDoPainel>) {
        let resumo = resumo_dos_paineis(&lista);
        let itens: Vec<PainelUi> = lista
            .iter()
            .map(|p| PainelUi {
                nome: p.nome.clone().into(),
                conectado: p.conectado,
            })
            .collect();
        self.paineis.replace(lista);
        self.com_janela(|janela| {
            let dados = janela.global::<Dados>();
            dados.set_paineis(ModelRc::new(VecModel::from(itens.clone())));
            dados.set_paineis_resumo(resumo.clone().into());
        });
    }

    fn mostrar_codigo_do_painel(&self, digitos: Option<[u8; 6]>) {
        let texto: Vec<SharedString> = match digitos {
            Some(digitos) => digitos.iter().map(|d| d.to_string().into()).collect(),
            None => vec![SharedString::new(); 6],
        };
        self.com_janela(|janela| {
            janela
                .global::<Dados>()
                .set_painel_digitos(ModelRc::new(VecModel::from(texto.clone())));
        });
    }

    fn etapa_do_painel(&self) -> EtapaDoPainel {
        self.janela
            .upgrade()
            .map_or(EtapaDoPainel::Parado, |janela| {
                janela.global::<Dados>().get_painel_etapa()
            })
    }
}
