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
//! - **só a última espera**: dois pedidos novos enquanto um copia deixam o segundo, não os dois.
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

/// O estado da fila de um.
#[derive(Debug, Default)]
struct Estado {
    /// O que espera a vez. No máximo um.
    pendente: Option<Trabalho>,
    /// O que está sendo copiado agora.
    em_curso: Option<Chave>,
    /// Se o que está em curso deve parar.
    cancelar: bool,
}

/// A fila de um, compartilhada entre quem pede e quem copia.
#[derive(Debug, Default)]
pub(crate) struct Fila {
    estado: Mutex<Estado>,
}

impl Fila {
    /// Pede uma cópia.
    pub(crate) fn pedir(&self, caminhos: Vec<PathBuf>, leitor: Leitor) -> Recebido {
        let chave = chave_de(&caminhos);
        let mut estado = self.travar();
        if estado.em_curso == Some(chave) {
            return Recebido::Repetido;
        }
        if estado.pendente.as_ref().map(|t| t.chave) == Some(chave) {
            return Recebido::Repetido;
        }
        estado.pendente = Some(Trabalho {
            caminhos,
            leitor,
            chave,
        });
        if estado.em_curso.is_some() {
            estado.cancelar = true;
            return Recebido::Substituiu;
        }
        Recebido::Aceito
    }

    /// Tira da fila o que espera, se houver, e marca que ele começou.
    pub(crate) fn tomar(&self) -> Option<Trabalho> {
        let mut estado = self.travar();
        let trabalho = estado.pendente.take()?;
        estado.em_curso = Some(trabalho.chave);
        estado.cancelar = false;
        Some(trabalho)
    }

    /// Tira o que espera sem começá-lo — para recusá-lo, quando não há par.
    pub(crate) fn descartar(&self) -> Option<Trabalho> {
        self.travar().pendente.take()
    }

    /// A pessoa pediu para parar: o que espera sai, e o que está indo para. `false` se não havia
    /// nada a parar.
    pub(crate) fn cancelar_tudo(&self) -> bool {
        let mut estado = self.travar();
        let havia = estado.pendente.take().is_some() || estado.em_curso.is_some();
        if estado.em_curso.is_some() {
            estado.cancelar = true;
        }
        havia
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
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    fn caminhos(nomes: &[&str]) -> Vec<PathBuf> {
        nomes.iter().map(PathBuf::from).collect()
    }

    fn pedir(fila: &Fila, nomes: &[&str]) -> Recebido {
        fila.pedir(caminhos(nomes), Leitor::Proprio)
    }

    #[test]
    fn a_mesma_pasta_pedida_de_novo_nao_vira_outra_copia() {
        // O defeito relatado: Ctrl+C três vezes na mesma pasta copiava três vezes.
        let fila = Fila::default();
        assert_eq!(pedir(&fila, &["/casa/teste"]), Recebido::Aceito);
        assert_eq!(pedir(&fila, &["/casa/teste"]), Recebido::Repetido);
        let trabalho = fila.tomar().expect("há o que copiar");
        assert_eq!(trabalho.caminhos, caminhos(&["/casa/teste"]));
        assert_eq!(
            pedir(&fila, &["/casa/teste"]),
            Recebido::Repetido,
            "agora está indo; pedir de novo não duplica"
        );
        assert!(fila.tomar().is_none(), "não há segunda cópia esperando");
        assert!(!fila.cancelando(), "nada a cancelar");
    }

    #[test]
    fn copiar_outra_coisa_cancela_a_que_esta_indo() {
        let fila = Fila::default();
        assert_eq!(pedir(&fila, &["/casa/teste"]), Recebido::Aceito);
        fila.tomar().expect("começou a primeira");
        assert_eq!(pedir(&fila, &["/casa/ffmpeg"]), Recebido::Substituiu);
        assert!(fila.cancelando(), "a que está indo precisa parar");

        fila.terminou();
        assert!(!fila.cancelando());
        let proxima = fila.tomar().expect("a nova assume");
        assert_eq!(proxima.caminhos, caminhos(&["/casa/ffmpeg"]));
    }

    #[test]
    fn cancelar_para_o_que_esta_indo_e_esquece_o_que_espera() {
        let fila = Fila::default();
        assert!(!fila.cancelar_tudo(), "sem cópia, nada a cancelar");
        pedir(&fila, &["/casa/a"]);
        fila.tomar().expect("começou");
        pedir(&fila, &["/casa/b"]);
        assert!(fila.cancelar_tudo());
        assert!(fila.cancelando(), "a que está indo para");
        fila.terminou();
        assert!(fila.tomar().is_none(), "a que esperava não começa depois");
    }

    #[test]
    fn entre_dois_pedidos_novos_so_o_ultimo_espera() {
        let fila = Fila::default();
        pedir(&fila, &["/casa/a"]);
        fila.tomar().expect("a primeira começou");
        pedir(&fila, &["/casa/b"]);
        pedir(&fila, &["/casa/c"]);
        fila.terminou();
        let proxima = fila.tomar().expect("há o que copiar");
        assert_eq!(proxima.caminhos, caminhos(&["/casa/c"]));
        assert!(fila.tomar().is_none(), "o do meio não ficou guardado");
    }

    #[test]
    fn depois_de_terminar_a_mesma_pasta_pode_ir_de_novo() {
        // Não é repetição acidental: a cópia acabou, e o usuário copiou de novo de propósito.
        let fila = Fila::default();
        pedir(&fila, &["/casa/teste"]);
        fila.tomar();
        fila.terminou();
        assert_eq!(pedir(&fila, &["/casa/teste"]), Recebido::Aceito);
    }

    #[test]
    fn a_ordem_dos_caminhos_faz_parte_do_pedido() {
        let fila = Fila::default();
        pedir(&fila, &["/casa/a", "/casa/b"]);
        fila.tomar();
        assert_eq!(
            pedir(&fila, &["/casa/b", "/casa/a"]),
            Recebido::Substituiu,
            "seleção diferente, ainda que dos mesmos itens"
        );
    }

    #[test]
    fn descartar_tira_o_que_espera_sem_comecar() {
        let fila = Fila::default();
        pedir(&fila, &["/casa/teste"]);
        let descartado = fila.descartar().expect("havia um esperando");
        assert_eq!(descartado.caminhos, caminhos(&["/casa/teste"]));
        assert!(fila.tomar().is_none());
    }
}
