//! Os painéis no Android pareados com este computador ([12](../../../docs/12-painel-android.md)).
//!
//! Um painel é um par que **só lê**: a chave dele fica aqui, longe de [`PinnedPeer`](crate::PinnedPeer),
//! para nenhum caminho que dá entrada, clipboard ou arquivo a um par conseguir achá-lo por engano.

use ir_crypto::PublicKey;
use serde::{Deserialize, Serialize};

use crate::{Config, decode_key, encode_key};

/// Um painel pareado.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PainelPareado {
    /// A chave pública do aparelho, em hexadecimal.
    pub pubkey: String,
    /// O nome que o aparelho disse, para a lista na janela.
    #[serde(default)]
    pub nome: String,
}

/// O nome de um painel que ainda não disse o dele.
pub const NOME_PROVISORIO: &str = "Painel";

impl Config {
    /// As chaves dos painéis pareados. Uma chave ilegível no arquivo fica de fora: o painel dela
    /// simplesmente não volta, e a pessoa pareia de novo.
    #[must_use]
    pub fn chaves_dos_paineis(&self) -> Vec<PublicKey> {
        self.paineis
            .iter()
            .filter_map(|painel| decode_key(&painel.pubkey))
            .collect()
    }

    /// Grava um painel recém-pareado. Devolve se mudou algo — um painel já gravado fica como está.
    pub fn parear_painel(&mut self, chave: &PublicKey) -> bool {
        let texto = encode_key(chave);
        if self.paineis.iter().any(|painel| painel.pubkey == texto) {
            return false;
        }
        self.paineis.push(PainelPareado {
            pubkey: texto,
            nome: NOME_PROVISORIO.to_owned(),
        });
        true
    }

    /// O nome que o painel disse. Devolve se mudou algo.
    pub fn nomear_painel(&mut self, chave: &PublicKey, nome: &str) -> bool {
        let texto = encode_key(chave);
        let nome = nome.trim();
        match self
            .paineis
            .iter_mut()
            .find(|painel| painel.pubkey == texto)
        {
            Some(painel) if !nome.is_empty() && painel.nome != nome => {
                nome.clone_into(&mut painel.nome);
                true
            }
            _ => false,
        }
    }

    /// Esquece o painel desta chave, em hexadecimal. Devolve se havia um.
    pub fn esquecer_painel(&mut self, chave: &str) -> bool {
        let antes = self.paineis.len();
        self.paineis.retain(|painel| painel.pubkey != chave);
        self.paineis.len() != antes
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ir_crypto::Identity;

    #[test]
    fn parear_nomear_e_esquecer() {
        let mut config = Config::default();
        let chave = Identity::generate().public();
        assert!(config.parear_painel(&chave));
        assert!(
            !config.parear_painel(&chave),
            "o mesmo painel não entra duas vezes"
        );
        assert_eq!(config.paineis[0].nome, NOME_PROVISORIO);

        assert!(config.nomear_painel(&chave, " Tablet da sala "));
        assert!(!config.nomear_painel(&chave, "Tablet da sala"));
        assert!(
            !config.nomear_painel(&chave, "  "),
            "nome vazio não apaga o que havia"
        );
        assert_eq!(config.chaves_dos_paineis(), vec![chave]);

        assert!(config.esquecer_painel(&encode_key(&chave)));
        assert!(config.paineis.is_empty());
        assert!(!config.esquecer_painel(&encode_key(&chave)));
    }

    #[test]
    fn um_arquivo_sem_paineis_continua_valendo() {
        let config: Config = toml::from_str(
            "peer_edge = \"right\"\nport = 52525\nscreen_width = 1\nscreen_height = 1\n",
        )
        .unwrap();
        assert!(config.paineis.is_empty());
        let texto = toml::to_string_pretty(&config).unwrap();
        assert!(!texto.contains("paineis"), "lista vazia não é gravada");
    }
}
