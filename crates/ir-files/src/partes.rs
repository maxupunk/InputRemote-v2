//! O manifesto que chega em partes, juntado de volta por quem recebe.
//!
//! Quem envia manda as [`BulkMessage::ManifestPart`] que o manifesto precisar e o
//! [`BulkMessage::Manifest`] por último ([`ir_proto::message::manifest_messages`]). Aqui elas
//! viram de novo a lista inteira, e só ela segue para [`crate::Recepcao::abrir`] — que confere
//! cota, caminhos e disco exatamente como conferia o manifesto de um quadro só.
//!
//! # O teto
//!
//! Partes vêm da rede, e um par hostil poderia mandá-las sem fim. A lista para de crescer um item
//! acima de [`limits::MAX_MANIFEST_ITEMS`]: é o bastante para a cota recusar com "itens demais",
//! que é a verdade, sem guardar o resto na memória.

use ir_proto::limits;
use ir_proto::message::{BulkMessage, ManifestItem, TransferId};

/// Um manifesto inteiro: a transferência, os itens e o total de bytes.
pub type Manifesto = (TransferId, Vec<ManifestItem>, u64);

/// O que uma mensagem fez com o manifesto em montagem.
#[derive(Debug, PartialEq, Eq)]
pub enum Juntada {
    /// Era uma parte; o manifesto ainda não fechou.
    Incompleto,
    /// O manifesto fechou, com todas as partes que vieram antes.
    Completo(Manifesto),
    /// Não é do manifesto: segue para quem cuida do resto.
    Outra(BulkMessage),
}

/// As partes já chegadas do manifesto em montagem.
#[derive(Debug, Default)]
pub struct PartesDoManifesto {
    /// De qual transferência são as partes guardadas.
    id: Option<TransferId>,
    itens: Vec<ManifestItem>,
}

impl PartesDoManifesto {
    /// Junta esta mensagem ao manifesto em montagem, se ela for dele.
    pub fn juntar(&mut self, mensagem: BulkMessage) -> Juntada {
        match mensagem {
            BulkMessage::ManifestPart { id, items } => {
                self.acrescentar(id, items);
                Juntada::Incompleto
            }
            BulkMessage::Manifest {
                id,
                items,
                total_bytes,
            } => {
                self.acrescentar(id, items);
                self.id = None;
                Juntada::Completo((id, std::mem::take(&mut self.itens), total_bytes))
            }
            outra => Juntada::Outra(outra),
        }
    }

    /// Guarda os itens de uma parte.
    ///
    /// Uma parte de **outra** transferência descarta o que havia: aquela foi abandonada antes de o
    /// manifesto fechar — cancelada, ou substituída por uma cópia nova —, e misturar as duas
    /// listas seria anunciar arquivos que ninguém vai mandar.
    fn acrescentar(&mut self, id: TransferId, itens: Vec<ManifestItem>) {
        if self.id != Some(id) {
            self.id = Some(id);
            self.itens.clear();
        }
        let teto = limits::MAX_MANIFEST_ITEMS + 1;
        let cabem = teto.saturating_sub(self.itens.len());
        self.itens.extend(itens.into_iter().take(cabem));
    }
}

#[cfg(test)]
mod testes {
    use super::*;

    fn item(nome: &str) -> ManifestItem {
        ManifestItem {
            path: nome.to_owned(),
            size: 1,
            is_dir: false,
        }
    }

    fn itens(nomes: &[&str]) -> Vec<ManifestItem> {
        nomes.iter().map(|nome| item(nome)).collect()
    }

    fn juntar_tudo(partes: &mut PartesDoManifesto, mensagens: Vec<BulkMessage>) -> Juntada {
        let mut ultima = Juntada::Incompleto;
        for mensagem in mensagens {
            ultima = partes.juntar(mensagem);
        }
        ultima
    }

    #[test]
    fn um_manifesto_de_um_quadro_fecha_na_hora() {
        let mut partes = PartesDoManifesto::default();
        let manifesto = BulkMessage::Manifest {
            id: TransferId(1),
            items: itens(&["a"]),
            total_bytes: 1,
        };
        assert_eq!(
            partes.juntar(manifesto),
            Juntada::Completo((TransferId(1), itens(&["a"]), 1))
        );
    }

    #[test]
    fn as_partes_voltam_a_ser_a_lista_inteira_na_ordem() {
        let mut partes = PartesDoManifesto::default();
        let id = TransferId(2);
        let mensagens = vec![
            BulkMessage::ManifestPart {
                id,
                items: itens(&["a", "b"]),
            },
            BulkMessage::ManifestPart {
                id,
                items: itens(&["c"]),
            },
            BulkMessage::Manifest {
                id,
                items: itens(&["d"]),
                total_bytes: 4,
            },
        ];
        assert_eq!(
            juntar_tudo(&mut partes, mensagens),
            Juntada::Completo((id, itens(&["a", "b", "c", "d"]), 4))
        );
    }

    #[test]
    fn o_que_nao_e_do_manifesto_segue_adiante() {
        let mut partes = PartesDoManifesto::default();
        let aceite = BulkMessage::Accept { id: TransferId(3) };
        assert_eq!(partes.juntar(aceite.clone()), Juntada::Outra(aceite));
    }

    #[test]
    fn as_partes_de_uma_transferencia_abandonada_nao_se_misturam_com_a_seguinte() {
        let mut partes = PartesDoManifesto::default();
        let mensagens = vec![
            BulkMessage::ManifestPart {
                id: TransferId(4),
                items: itens(&["velho"]),
            },
            BulkMessage::Manifest {
                id: TransferId(5),
                items: itens(&["novo"]),
                total_bytes: 1,
            },
        ];
        assert_eq!(
            juntar_tudo(&mut partes, mensagens),
            Juntada::Completo((TransferId(5), itens(&["novo"]), 1))
        );
    }

    #[test]
    fn partes_sem_fim_param_de_crescer_logo_acima_do_limite() {
        // Um par hostil manda partes sem parar: a memória não acompanha, e a cota ainda vê itens
        // demais para recusar com o motivo certo.
        let mut partes = PartesDoManifesto::default();
        let id = TransferId(6);
        for _ in 0..3 {
            let lote = vec![item("x"); limits::MAX_MANIFEST_ITEMS];
            partes.juntar(BulkMessage::ManifestPart { id, items: lote });
        }
        let fim = BulkMessage::Manifest {
            id,
            items: itens(&["y"]),
            total_bytes: 0,
        };
        let Juntada::Completo((_, lista, _)) = partes.juntar(fim) else {
            panic!("o manifesto fechou");
        };
        assert_eq!(lista.len(), limits::MAX_MANIFEST_ITEMS + 1);
    }
}
