//! Uma cópia de cada vez, e a última é a que vale.
//!
//! Antes cada pedido entrava numa fila sem limite e todos aconteciam, um depois do outro. Apertar
//! Ctrl+C três vezes na mesma pasta de 2 GB mandava a pasta **três vezes** — e como nada aparecia
//! na tela, apertar de novo era justamente o que a pessoa fazia. Na bancada: dois envios iguais de
//! 2,2 GB seguidos, o segundo começando 7 ms depois de o primeiro terminar.
//!
//! As regras, que são de produto e por isso vivem aqui, testadas:
//!
//! - **a mesma coisa não vai duas vezes**: se o que está indo, ou o que está esperando, é isto,
//!   o pedido é aceito e não vira outra cópia;
//! - **coisa diferente substitui**: o que está indo é cancelado — o outro lado apaga o que já
//!   gravou, porque a montagem dele é temporária até o fim — e a nova começa;
//! - **só a última espera**: dois pedidos novos enquanto um copia deixam o segundo, não os dois;
//! - **a que o enlace interrompeu volta para a fila** ([`Fila::devolver`]) e vai de novo quando ele
//!   voltar — a menos que a pessoa a tenha cancelado ou copiado outra coisa nesse meio.
//!
//! O que identifica "a mesma coisa" é o conjunto de caminhos pedidos, e não o conteúdo deles: é o
//! que o usuário selecionou, é barato de comparar, e um arquivo editado entre dois Ctrl+C continua
//! sendo o mesmo pedido — copiar de novo é o que ele quer quando termina e copia outra vez.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;
use std::sync::Mutex;

use ir_files::Leitor;

/// O que identifica um pedido de cópia.
pub(crate) type Chave = u64;

/// Uma cópia pedida: o que copiar, e com a autoridade de quem.
#[derive(Debug, Clone)]
pub(crate) struct Trabalho {
    pub(crate) caminhos: Vec<PathBuf>,
    pub(crate) leitor: Leitor,
    pub(crate) chave: Chave,
    /// Quantas vezes o enlace já caiu no meio desta cópia.
    pub(crate) quedas: u8,
    /// Até onde ela tinha ido quando o enlace caiu: bytes feitos e bytes no total.
    pub(crate) progresso: (u64, u64),
    /// Se ela está na tela esperando a conexão. Quem a tirar da fila sem começá-la tem de dizer o
    /// que houve com ela, senão o cartão fica esperando para sempre.
    a_vista: bool,
}

/// Uma cópia que espera a conexão, do jeito que estava quando foi mostrada.
///
/// É o que permite desistir **dela**, e só dela, quando o prazo acabar: se outra cópia tomou o
/// lugar, ou se ela já recomeçou, a desistência não acha nada.
#[derive(Debug, Clone)]
pub(crate) struct Espera {
    pub(crate) caminhos: Vec<PathBuf>,
    pub(crate) progresso: (u64, u64),
    chave: Chave,
    inicios: u64,
}

/// O que aconteceu com o pedido.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Recebido {
    /// Entrou, e é o próximo.
    Aceito,
    /// É o que já está indo, ou o que já está esperando: não vira outra cópia.
    Repetido,
    /// Entrou no lugar do que estava indo, que será cancelado.
    Substituiu,
}

/// O que aconteceu com a cópia que o enlace interrompeu.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Devolucao {
    /// Voltou para a fila: vai de novo quando o enlace voltar.
    VaiDeNovo,
    /// Foi cancelada, ou outra tomou o lugar dela, enquanto ia: não volta.
    Cancelada,
}

/// O estado da fila de um.
#[derive(Debug, Default)]
struct Estado {
    /// O que espera a vez. No máximo um.
    pendente: Option<Trabalho>,
    /// O que está sendo copiado agora — inteiro, e não só a chave, para ser recuperado se quem o
    /// conduzia for largado no meio ([`Fila::abandonada`]).
    em_curso: Option<Trabalho>,
    /// Se o que está em curso deve parar.
    cancelar: bool,
    /// Quantas cópias já começaram. Muda a cada [`Fila::tomar`], e é o que diz a uma desistência
    /// atrasada que a cópia que ela vigiava já recomeçou.
    inicios: u64,
}

/// A fila de um, compartilhada entre quem pede e quem copia.
#[derive(Debug, Default)]
pub(crate) struct Fila {
    estado: Mutex<Estado>,
}

impl Fila {
    /// Pede uma cópia.
    ///
    /// Devolve também a cópia que esperava a conexão **na tela** e saiu da fila para dar lugar a
    /// esta: quem pediu diz que ela não vai mais.
    pub(crate) fn pedir(
        &self,
        caminhos: Vec<PathBuf>,
        leitor: Leitor,
    ) -> (Recebido, Option<Trabalho>) {
        let chave = chave_de(&caminhos);
        let mut estado = self.travar();
        if estado.em_curso.as_ref().map(|t| t.chave) == Some(chave) {
            return (Recebido::Repetido, None);
        }
        if estado.pendente.as_ref().map(|t| t.chave) == Some(chave) {
            return (Recebido::Repetido, None);
        }
        let novo = Trabalho {
            caminhos,
            leitor,
            chave,
            quedas: 0,
            progresso: (0, 0),
            a_vista: false,
        };
        let saiu = estado.pendente.replace(novo).filter(|t| t.a_vista);
        if estado.em_curso.is_some() {
            estado.cancelar = true;
            return (Recebido::Substituiu, saiu);
        }
        (Recebido::Aceito, saiu)
    }

    /// Tira da fila o que espera, se houver, e marca que ele começou.
    pub(crate) fn tomar(&self) -> Option<Trabalho> {
        let mut estado = self.travar();
        let mut trabalho = estado.pendente.take()?;
        trabalho.a_vista = false;
        estado.em_curso = Some(trabalho.clone());
        estado.cancelar = false;
        estado.inicios = estado.inicios.wrapping_add(1);
        Some(trabalho)
    }

    /// Tira o que espera sem começá-lo — para recusá-lo, quando não há par.
    pub(crate) fn descartar(&self) -> Option<Trabalho> {
        self.travar().pendente.take()
    }

    /// A pessoa pediu para parar: o que espera sai, e o que está indo para. `false` se não havia
    /// nada a parar.
    ///
    /// Devolve também a cópia que esperava a conexão **na tela**, para quem cancelou dizer que
    /// ela parou. A que está indo diz por conta própria, quando para.
    pub(crate) fn cancelar_tudo(&self) -> (bool, Option<Trabalho>) {
        let mut estado = self.travar();
        let pendente = estado.pendente.take();
        let havia = pendente.is_some() || estado.em_curso.is_some();
        if estado.em_curso.is_some() {
            estado.cancelar = true;
        }
        (havia, pendente.filter(|t| t.a_vista))
    }

    /// Se a cópia em curso deve parar para dar lugar a outra.
    pub(crate) fn cancelando(&self) -> bool {
        self.travar().cancelar
    }

    /// A cópia em curso acabou, de um jeito ou de outro.
    pub(crate) fn terminou(&self) {
        let mut estado = self.travar();
        estado.em_curso = None;
        estado.cancelar = false;
    }

    /// A cópia em curso foi interrompida pela queda do enlace: volta a esperar a vez, a menos que
    /// a pessoa a tenha cancelado, ou copiado outra coisa, enquanto ela ia.
    pub(crate) fn devolver(&self, trabalho: Trabalho) -> Devolucao {
        let mut estado = self.travar();
        let cancelada = estado.cancelar || estado.pendente.is_some();
        estado.em_curso = None;
        estado.cancelar = false;
        if cancelada {
            return Devolucao::Cancelada;
        }
        estado.pendente = Some(trabalho);
        Devolucao::VaiDeNovo
    }

    /// A cópia que estava em curso quando quem a conduzia foi largado no meio — o canal recomeçou
    /// porque o par mudou —, sem chance de dizer como ela terminou. `None` se nenhuma ficou.
    ///
    /// Sem isto ela ficava marcada como em curso para sempre, e pedir a mesma cópia de novo era
    /// tomado por Ctrl+C repetido: nada saía, e nada dizia por quê.
    pub(crate) fn abandonada(&self) -> Option<Trabalho> {
        let mut estado = self.travar();
        estado.cancelar = false;
        estado.em_curso.take()
    }

    /// Marca a cópia que espera como mostrada na tela, e devolve como ela está.
    ///
    /// `None` se não há nenhuma, ou se ela já estava à vista: o aviso e o prazo dela já correm, e
    /// um segundo par dos dois só faria barulho.
    pub(crate) fn mostrar_espera(&self) -> Option<Espera> {
        let mut estado = self.travar();
        let inicios = estado.inicios;
        let trabalho = estado.pendente.as_mut().filter(|t| !t.a_vista)?;
        trabalho.a_vista = true;
        Some(Espera {
            caminhos: trabalho.caminhos.clone(),
            progresso: trabalho.progresso,
            chave: trabalho.chave,
            inicios,
        })
    }

    /// Desiste da cópia que esperava, se ela ainda é a mesma e ainda não recomeçou.
    pub(crate) fn desistir(&self, espera: &Espera) -> Option<Trabalho> {
        let mut estado = self.travar();
        let mesma = estado.inicios == espera.inicios
            && estado.pendente.as_ref().map(|t| t.chave) == Some(espera.chave);
        if mesma { estado.pendente.take() } else { None }
    }

    /// O cadeado, com o envenenamento tratado: uma linha de cópia não derruba o serviço.
    fn travar(&self) -> std::sync::MutexGuard<'_, Estado> {
        self.estado
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

/// O identificador do conjunto de caminhos, na ordem em que vieram.
fn chave_de(caminhos: &[PathBuf]) -> Chave {
    let mut hasher = DefaultHasher::new();
    caminhos.hash(&mut hasher);
    hasher.finish()
}

#[cfg(test)]
mod testes;
