//! A tela de pareamento ligada ao serviço: procurar, escolher ou digitar, e comparar o código.
//!
//! O fluxo que a tela promete: abrir "Parear" nos dois computadores, cada um vê o outro, clica-se em
//! qualquer um dos lados, e o outro **vai sozinho** para os seis dígitos — mesmo escondido na bandeja.
//! Nada aqui decide se algo pode ser feito; isso é do serviço. Aqui é só tradução entre a tela e os
//! pedidos e avisos.

use std::rc::Rc;

use ir_ipc::{Candidato, Falha, Pedido, Resposta};
use slint::{ComponentHandle, ModelRc, SharedString, VecModel};

use super::{Contexto, seis_vazios};
use crate::gerado::{Acoes, Dados, EtapaDoPareamento, Janela};
use crate::ponte;

impl Contexto {
    pub(super) fn mostrar_candidatos(&self, candidatos: Vec<Candidato>) {
        let rotulos: Vec<SharedString> = candidatos
            .iter()
            .map(|candidato| {
                // O nome sozinho não distingue o mesmo computador achado por dois meios.
                let meio = candidato.portador.nome();
                SharedString::from(format!("{} · {meio}", candidato.rotulo))
            })
            .collect();
        *self.candidatos.borrow_mut() = candidatos;
        self.com_janela(|janela| {
            let dados = janela.global::<Dados>();
            dados.set_candidatos(ModelRc::new(VecModel::from(rotulos.clone())));
            dados.set_procurando(false);
        });
    }

    /// Chegou um código: a tela vai para a comparação, esteja onde estiver.
    ///
    /// Quem recebe o pedido de pareamento não clicou em nada. Antes, o código ficava num estado que
    /// ninguém via — a janela seguia na tela inicial, ou escondida na bandeja —, e a pessoa do outro
    /// lado esperava por uma comparação que não ia acontecer.
    pub(super) fn mostrar_codigo(&self, digitos: [u8; 6]) {
        let caixas: Vec<SharedString> = digitos
            .iter()
            .map(|digito| SharedString::from(digito.to_string()))
            .collect();
        self.com_janela(|janela| {
            janela
                .global::<Dados>()
                .set_digitos(ModelRc::new(VecModel::from(caixas.clone())));
            janela.invoke_mostrar_pareamento();
            let janela = janela.window();
            janela.set_minimized(false);
            let _ = janela.show();
        });
        self.etapa(EtapaDoPareamento::Comparando);
    }

    /// Pede o pareamento a um endereço, e a tela passa a esperar o outro lado — se o serviço
    /// aceitou o pedido.
    ///
    /// A tela ia para "Chamando o outro computador…" mesmo quando o serviço recusava (um endereço
    /// de Bluetooth numa máquina sem rádio, por exemplo), e ficava ali para sempre: o aviso que ela
    /// prometia nunca vinha, porque o pedido nem tinha saído.
    fn parear_com(&self, endereco: String) {
        match self.servico.pedir(Pedido::IniciarPareamento {
            candidato: endereco,
        }) {
            Resposta::Falha(falha) => self.falha_no_pareamento(falha),
            _ => self.etapa(EtapaDoPareamento::Esperando),
        }
    }
}

pub(super) fn ligar(janela: &Janela, contexto: &Rc<Contexto>) {
    ligar_busca(janela, contexto);
    ligar_escolha(janela, contexto);
    let acoes = janela.global::<Acoes>();

    let alvo = Rc::clone(contexto);
    acoes.on_confirmar_pareamento(move |conferiu| {
        // A recusa vai para a tela de pareamento, e não para o recado da janela: o usuário está
        // dentro de um fluxo, e tirá-lo dali para mostrar um aviso avulso perderia o contexto.
        if let Resposta::Falha(falha) = alvo.servico.pedir(Pedido::ConfirmarPareamento { conferiu })
        {
            alvo.falha_no_pareamento(falha);
        } else {
            // Deste lado confere; falta o outro computador. Ficar na comparação depois do clique
            // fazia parecer que o botão não tinha funcionado (log 25).
            if conferiu {
                alvo.etapa(EtapaDoPareamento::Esperando);
            }
            alvo.sincronizar();
        }
    });

    let alvo = Rc::clone(contexto);
    acoes.on_reiniciar_pareamento(move || {
        // Um código em comparação não se apaga por entrar de novo na tela: o outro computador está
        // mostrando os mesmos dígitos agora.
        if alvo.etapa_atual() == Some(EtapaDoPareamento::Comparando) {
            return;
        }
        alvo.com_janela(|janela| {
            let dados = janela.global::<Dados>();
            dados.set_erro(SharedString::new());
            dados.set_erro_o_que_fazer(SharedString::new());
            dados.set_digitos(seis_vazios());
            dados.set_endereco_erro(SharedString::new());
        });
        alvo.etapa(EtapaDoPareamento::Escolhendo);
    });
}

/// Com quem parear: um da lista, ou o endereço digitado.
fn ligar_escolha(janela: &Janela, contexto: &Rc<Contexto>) {
    let acoes = janela.global::<Acoes>();

    let alvo = Rc::clone(contexto);
    acoes.on_iniciar_pareamento(move |posicao| {
        let escolhido = usize::try_from(posicao)
            .ok()
            .and_then(|posicao| alvo.candidatos.borrow().get(posicao).cloned());
        match escolhido {
            Some(candidato) => alvo.parear_com(candidato.endereco),
            None => alvo.recado(Some(Falha::ForaDeContexto)),
        }
    });

    let alvo = Rc::clone(contexto);
    acoes.on_parear_endereco(
        move |digitado| match ponte::ler_endereco_digitado(&digitado) {
            Ok(endereco) => {
                alvo.com_janela(|janela| janela.global::<Dados>().set_endereco_erro("".into()));
                alvo.parear_com(endereco);
            }
            Err(motivo) => alvo.com_janela(|janela| {
                janela.global::<Dados>().set_endereco_erro(motivo.into());
            }),
        },
    );
}

/// "Procurar de novo", e a busca de fundo que a tela faz sozinha enquanto a lista está aberta.
fn ligar_busca(janela: &Janela, contexto: &Rc<Contexto>) {
    let acoes = janela.global::<Acoes>();

    let alvo = Rc::clone(contexto);
    acoes.on_procurar(move || {
        alvo.com_janela(|janela| {
            let dados = janela.global::<Dados>();
            dados.set_procurando(true);
            dados.set_candidatos(ModelRc::new(VecModel::from(Vec::<SharedString>::new())));
        });
        alvo.enviar(Pedido::Procurar);
    });

    // Sem apagar a lista nem mostrar "Procurando…": o outro computador aparece quando é aberto, e
    // quem já está na lista não pisca.
    let alvo = Rc::clone(contexto);
    acoes.on_atualizar_busca(move || {
        let _ = alvo.servico.pedir(Pedido::Procurar);
    });
}
