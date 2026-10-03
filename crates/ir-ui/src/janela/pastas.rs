//! A tela das pastas compartilhadas ligada aos pedidos.
//!
//! Só tradução: o clique vira [`ComandoDePasta`], e a lista que volta do serviço vira [`PastaUi`].
//! A tela escolhe por posição, e é aqui que a posição vira a pasta — o identificador não sai do
//! Rust, como os candidatos do pareamento.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::mpsc::{Receiver, TryRecvError};

use ir_ipc::pastas::{
    ComandoDePasta, EscolhaDeConflito, ResumoDePasta, SituacaoDaPasta, frase_do_conflito,
    resumo_das_pastas,
};
use ir_ipc::{Pedido, Resposta};
use slint::{ComponentHandle, ModelRc, SharedString, VecModel};

use super::Contexto;
use crate::gerado::{Acoes, ConflitoUi, Dados, Janela, PastaUi};

/// O que a janela guarda das pastas.
#[derive(Default)]
pub(super) struct EstadoDasPastas {
    /// A lista como a tela a mostra, na mesma ordem.
    lista: RefCell<Vec<ResumoDePasta>>,
    /// O seletor de pasta aberto, quando há um: ele roda fora da janela, e a resposta chega aqui.
    escolha: RefCell<Option<Receiver<Option<String>>>>,
}

pub(super) fn ligar(janela: &Janela, contexto: &Rc<Contexto>) {
    let acoes = janela.global::<Acoes>();

    let alvo = Rc::clone(contexto);
    acoes.on_compartilhar_pasta(move || {
        if alvo.pastas.escolha.borrow().is_none() {
            alvo.pastas
                .escolha
                .replace(Some(crate::escolher_pasta::abrir()));
        }
    });

    let alvo = Rc::clone(contexto);
    acoes.on_criar_pasta(move |nome| {
        let nome = nome.trim().to_owned();
        if !nome.is_empty() {
            alvo.comando_de_pasta(ComandoDePasta::Criar { nome });
        }
    });

    acoes.on_aceitar_pasta(por_posicao(contexto, ComandoDePasta::Aceitar));
    acoes.on_recusar_pasta(por_posicao(contexto, ComandoDePasta::Recusar));
    acoes.on_parar_pasta(por_posicao(contexto, ComandoDePasta::Parar));
    acoes.on_abrir_pasta(por_posicao(contexto, ComandoDePasta::Abrir));
    acoes.on_abrir_lixeira(por_posicao(contexto, ComandoDePasta::AbrirLixeira));

    let alvo = Rc::clone(contexto);
    acoes.on_resolver_conflito(move |posicao, indice, escolha| {
        let comando = {
            let lista = alvo.pastas.lista.borrow();
            let pasta = usize::try_from(posicao).ok().and_then(|i| lista.get(i));
            let conflito = pasta.and_then(|p| {
                usize::try_from(indice)
                    .ok()
                    .and_then(|i| p.lista_de_conflitos.get(i))
                    .map(|c| (p.id, c.original.clone()))
            });
            conflito.map(|(pasta, caminho)| ComandoDePasta::Resolver {
                pasta,
                caminho,
                escolha: match escolha {
                    1 => EscolhaDeConflito::FicarComEsta,
                    2 => EscolhaDeConflito::FicarComAOutra,
                    _ => EscolhaDeConflito::ManterAsDuas,
                },
            })
        };
        if let Some(comando) = comando {
            alvo.comando_de_pasta(comando);
        }
    });
}

/// O clique numa posição da lista vira o comando para a pasta que está nela.
fn por_posicao(
    contexto: &Rc<Contexto>,
    montar: fn(ir_ipc::pastas::IdDePasta) -> ComandoDePasta,
) -> impl Fn(i32) + 'static {
    let alvo = Rc::clone(contexto);
    move |posicao| {
        let id = usize::try_from(posicao)
            .ok()
            .and_then(|i| alvo.pastas.lista.borrow().get(i).map(|p| p.id));
        if let Some(id) = id {
            alvo.comando_de_pasta(montar(id));
        }
    }
}

impl Contexto {
    /// Manda um pedido sobre as pastas. A lista nova chega pelo aviso.
    fn comando_de_pasta(&self, comando: ComandoDePasta) {
        match self.servico.pedir(Pedido::Pasta(comando)) {
            Resposta::Falha(falha) => self.recado(Some(falha)),
            _ => self.recado(None),
        }
    }

    /// Pede a lista ao serviço — ao abrir e ao reconectar.
    pub(super) fn buscar_pastas(&self) {
        if let Resposta::Pastas(lista) = self.servico.pedir(Pedido::Pastas) {
            self.mostrar_pastas(lista);
        }
    }

    /// Põe a lista na tela.
    pub(super) fn mostrar_pastas(&self, lista: Vec<ResumoDePasta>) {
        let resumo = resumo_das_pastas(&lista);
        let ofertas = lista
            .iter()
            .filter(|p| p.situacao == SituacaoDaPasta::Oferecida && p.caminho_local.is_empty())
            .count();
        let itens: Vec<PastaUi> = lista.iter().map(pasta_ui).collect();
        self.avisar_ofertas_novas(&lista);
        self.pastas.lista.replace(lista);
        self.com_janela(|janela| {
            let dados = janela.global::<Dados>();
            dados.set_pastas(ModelRc::new(VecModel::from(itens.clone())));
            dados.set_pastas_resumo(resumo.clone().into());
            dados.set_pastas_ofertas(i32::try_from(ofertas).unwrap_or(i32::MAX));
        });
    }

    /// Uma oferta que não estava na lista anterior vira aviso no canto da tela, no Windows: quem
    /// compartilhou está no outro computador, e a janela daqui pode estar fechada. No Linux quem
    /// avisa é o ajudante, pelo `notify-send`.
    #[cfg_attr(not(windows), allow(clippy::unused_self))]
    fn avisar_ofertas_novas(&self, lista: &[ResumoDePasta]) {
        #[cfg(windows)]
        {
            let anteriores = self.pastas.lista.borrow();
            let nova = lista.iter().find(|p| {
                p.situacao == SituacaoDaPasta::Oferecida
                    && p.caminho_local.is_empty()
                    && !anteriores.iter().any(|a| a.id == p.id)
            });
            if let Some(oferta) = nova {
                let aviso = crate::gerado::CopiaUi {
                    titulo: "Uma pasta compartilhada com você".into(),
                    detalhe: format!(
                        "O outro computador quer compartilhar \"{}\". Abra o InputRemote para aceitar.",
                        oferta.nome
                    )
                    .into(),
                    progresso: 1.0,
                    estado: 1,
                    velocidade: SharedString::new(),
                    cancelavel: false,
                    recebida: false,
                };
                self.aviso.borrow_mut().mostrar(aviso, true);
            }
        }
        #[cfg(not(windows))]
        let _ = lista;
    }

    /// Um recado que veio do ajudante das pastas, já com o que fazer.
    pub(super) fn recado_das_pastas(&self, frase: &str) {
        self.com_janela(|janela| {
            let dados = janela.global::<Dados>();
            dados.set_recado(frase.into());
            dados.set_recado_o_que_fazer(SharedString::new());
            dados.set_recado_informativo(false);
        });
    }

    /// Se o seletor de pasta respondeu, compartilha o que foi escolhido.
    pub(super) fn conferir_escolha(&self) {
        let resposta = match self
            .pastas
            .escolha
            .borrow()
            .as_ref()
            .map(Receiver::try_recv)
        {
            Some(Ok(resposta)) => resposta,
            Some(Err(TryRecvError::Empty)) | None => return,
            Some(Err(TryRecvError::Disconnected)) => None,
        };
        self.pastas.escolha.replace(None);
        if let Some(caminho) = resposta {
            self.comando_de_pasta(ComandoDePasta::Compartilhar { caminho });
        }
    }
}

fn pasta_ui(pasta: &ResumoDePasta) -> PastaUi {
    let lista: Vec<ConflitoUi> = pasta
        .lista_de_conflitos
        .iter()
        .map(|conflito| ConflitoUi {
            texto: frase_do_conflito(conflito).into(),
            de_quem: quem_fez_a_copia(&conflito.copia).into(),
        })
        .collect();
    PastaUi {
        nome: pasta.nome.clone().into(),
        linha: pasta.linha().into(),
        detalhe: pasta.detalhe().into(),
        conflitos: pasta.sobre_conflitos().into(),
        lista: ModelRc::new(VecModel::from(lista)),
        saude: pasta.saude(),
        oferta: pasta.situacao == SituacaoDaPasta::Oferecida && pasta.caminho_local.is_empty(),
    }
}

/// O computador que fez a cópia de conflito, lido do nome dela: `nome (conflito NOTEBOOK 2026-…)`.
///
/// O que vem antes da data. Contar espaços do fim não serve: a cópia numerada (`… 14h30 2)`) tem
/// um a mais, e o nome da máquina pode ter espaços.
fn quem_fez_a_copia(copia: &str) -> String {
    let eh_data = |palavra: &str| {
        palavra.len() == 10
            && palavra.char_indices().all(|(i, c)| {
                if i == 4 || i == 7 {
                    c == '-'
                } else {
                    c.is_ascii_digit()
                }
            })
    };
    copia
        .rsplit_once("(conflito ")
        .map(|(_, resto)| {
            resto
                .split(' ')
                .take_while(|p| !eh_data(p))
                .collect::<Vec<_>>()
                .join(" ")
        })
        .filter(|maquina| !maquina.is_empty() && !maquina.contains(')'))
        .unwrap_or_else(|| "outra".to_owned())
}

#[cfg(test)]
mod testes {
    use super::quem_fez_a_copia;

    #[test]
    fn o_computador_sai_do_nome_da_copia() {
        assert_eq!(
            quem_fez_a_copia("a/notas (conflito NOTEBOOK 2026-10-02 14h30).txt"),
            "NOTEBOOK"
        );
        assert_eq!(
            quem_fez_a_copia("x (conflito PC da sala 2026-10-02 14h30 2).doc"),
            "PC da sala"
        );
        assert_eq!(quem_fez_a_copia("sem padrão.txt"), "outra");
    }
}
