//! O recado do painel entre os dois computadores — o que vai dentro de `Control::Panel`.
//!
//! O computador que tem um painel olhando pede ao par a amostra dele, e os processos que o painel
//! quer ver; o par responde a cada segundo. O pedido **expira** em 5 s sem renovação, então quem
//! pede renova a cada 2 s, e um pedido de parar perdido não deixa ninguém coletando.
//!
//! Tudo aqui cabe em [`ir_proto::limits::MAX_PANEL_MESSAGE`], o teto do quadro do rádio: quando a
//! lista de processos não cabe, ela encolhe — a amostra e os vigiados vão sempre.

use ir_proto::limits::MAX_PANEL_MESSAGE;
use ir_proto::message::UsageSample;
use serde::{Deserialize, Serialize};

use crate::mensagem::{codificar as em_bytes, decodificar as de_bytes};
use crate::processos::{InteresseEmProcessos, Processos};

/// O recado.
///
/// A ordem das variantes **é o formato de fio**; acrescentar no fim é seguro. Vetores gravados em
/// `tests/vetores.rs`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum EntrePcs {
    /// Mande a sua amostra a cada segundo, com os processos deste interesse; vale por 5 s.
    Assinar(InteresseEmProcessos),
    /// Pare de mandar.
    Parar,
    /// A amostra de quem envia, e os processos pedidos.
    Uso(UsageSample, Option<Processos>),
}

/// O recado em bytes, dentro do teto do rádio. Uma lista de processos que não cabe encolhe até
/// caber; sem nada a encolher, `None`.
#[must_use]
pub fn codificar(recado: &EntrePcs) -> Option<Vec<u8>> {
    let mut recado = recado.clone();
    loop {
        let bytes = em_bytes(&recado).ok()?;
        if bytes.len() <= MAX_PANEL_MESSAGE {
            return Some(bytes);
        }
        let EntrePcs::Uso(_, Some(processos)) = &mut recado else {
            return None;
        };
        processos.lista.pop()?;
    }
}

/// Os bytes num recado, com a amostra e os processos já nos tetos. `None` para o que não é recado
/// desta versão — de um par mais novo, por exemplo: ele é ignorado, e não derruba nada.
#[must_use]
pub fn decodificar(bytes: &[u8]) -> Option<EntrePcs> {
    Some(match de_bytes::<EntrePcs>(bytes).ok()? {
        EntrePcs::Assinar(interesse) => EntrePcs::Assinar(interesse.nos_tetos()),
        EntrePcs::Uso(amostra, processos) => {
            EntrePcs::Uso(amostra.bounded(), processos.map(Processos::nos_tetos))
        }
        EntrePcs::Parar => EntrePcs::Parar,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::processos::{MAX_LISTA, MAX_VIGIADOS, NOME_MAXIMO, UsoDeProcesso};

    fn pesado() -> UsoDeProcesso {
        UsoDeProcesso {
            nome: "x".repeat(NOME_MAXIMO),
            instancias: u16::MAX,
            cpu_permille: 1000,
            memoria_kib: u64::MAX,
        }
    }

    #[test]
    fn o_maior_recado_possivel_cabe_no_radio() {
        let processos = Processos {
            vigiados: vec![pesado(); MAX_VIGIADOS],
            lista: vec![pesado(); MAX_LISTA],
        };
        let recado = EntrePcs::Uso(UsageSample::default(), Some(processos));
        let bytes = codificar(&recado).expect("encolhe até caber");
        assert!(bytes.len() <= MAX_PANEL_MESSAGE);
        let Some(EntrePcs::Uso(_, Some(volta))) = decodificar(&bytes) else {
            panic!("volta como uso");
        };
        assert_eq!(volta.vigiados.len(), MAX_VIGIADOS, "os vigiados vão sempre");
    }

    #[test]
    fn ida_e_volta() {
        let pedido = EntrePcs::Assinar(InteresseEmProcessos {
            nomes: vec!["docker".to_owned()],
            lista: true,
        });
        assert_eq!(decodificar(&codificar(&pedido).unwrap()), Some(pedido));
        assert_eq!(decodificar(&[0xFF, 0xFF]), None);
    }
}
