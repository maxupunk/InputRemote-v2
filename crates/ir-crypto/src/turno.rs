//! De quem é a vez de discar, quando os dois lados sabem o endereço do outro.
//!
//! # O defeito
//!
//! Com o `peer_addr` gravado dos dois lados — a configuração normal depois de parear —, os dois
//! serviços tentam reconectar a cada rodada de 3 s. O iniciador espera a resposta no mesmo socket
//! em que o par está mandando o **início** do handshake dele; lê esse início como se fosse a
//! resposta, e os dois falham. As rodadas têm o mesmo período, então quem colidiu uma vez tende a
//! colidir em toda rodada: na bancada, a sessão ficou minutos sem firmar.
//!
//! # A regra
//!
//! O lado de **chave pública maior** disca em toda rodada. O de chave menor disca na primeira e
//! depois só a cada [`RODADAS_DO_MENOR`]. Numa rodada em que os dois colidiram, a seguinte é só do
//! maior, e ela firma.
//!
//! O menor não fica calado de vez porque pode ser o único que sabe o endereço do outro — é o caso de
//! um Windows numa rede marcada como Pública, que não aceita conexão de entrada. Ele reconecta, só
//! que mais devagar.
//!
//! É a mesma ordem de chaves da regra de colisão do canal de arquivos
//! (`ir_net::bulk::keep_outbound_on_collision`): os dois lados chegam à mesma conclusão sem trocar
//! mensagem nenhuma.
//!
//! # Por que mora aqui
//!
//! Nasceu no `ir-net`, e saiu dele quando o Bluetooth passou a precisar da mesma regra: com a rota
//! dupla, os dois lados ficam sabendo o endereço de rádio do outro no mesmo instante, e discam o
//! RFCOMM juntos. Uma regra por portador divergiria; a regra é sobre as chaves, e as chaves são
//! deste crate ([ADR-0012](../../../docs/adr/0012-rota-dupla.md)).

use crate::enlace::ConnectMode;
use crate::identity::PublicKey;

/// A cada quantas rodadas o lado de chave menor disca.
pub const RODADAS_DO_MENOR: u32 = 3;

/// Se este lado disca na rodada `rodada` (a primeira é 1) desde o último enlace.
#[must_use]
pub fn discar_nesta_rodada(local: PublicKey, par: PublicKey, rodada: u32) -> bool {
    local.precede(&par) || rodada % RODADAS_DO_MENOR == 1
}

/// A contagem de rodadas de um endpoint, desde o último enlace.
///
/// Os dois endpoints — rede e rádio — faziam a mesma conta à mão: contar a rodada, perguntar se é a
/// vez, zerar quando um enlace firma. Aqui ela existe uma vez.
#[derive(Debug, Clone, Copy, Default)]
pub struct Rodadas(u32);

impl Rodadas {
    /// Se este pedido de conexão deve discar agora.
    ///
    /// Só a reconexão entra na regra e conta rodada: parear e trocar chaves são pedidos de quem
    /// está na frente da tela, ou de um enlace que já está de pé, e saem sempre.
    pub fn discar(&mut self, local: PublicKey, modo: ConnectMode) -> bool {
        let ConnectMode::Reconnect(par) = modo else {
            return true;
        };
        self.0 = self.0.wrapping_add(1);
        discar_nesta_rodada(local, par, self.0)
    }

    /// Um enlace firmou: a próxima queda começa a contar do zero.
    pub fn zerar(&mut self) {
        self.0 = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MAIOR: PublicKey = PublicKey([9; 32]);
    const MENOR: PublicKey = PublicKey([4; 32]);

    #[test]
    fn o_maior_disca_sempre() {
        assert!((1..=12).all(|rodada| discar_nesta_rodada(MAIOR, MENOR, rodada)));
    }

    #[test]
    fn o_menor_disca_na_primeira_e_depois_de_tempos_em_tempos() {
        let rodadas: Vec<u32> = (1..=10)
            .filter(|&rodada| discar_nesta_rodada(MENOR, MAIOR, rodada))
            .collect();
        assert_eq!(rodadas, vec![1, 4, 7, 10]);
    }

    #[test]
    fn depois_de_uma_colisao_a_rodada_seguinte_e_so_de_um() {
        // Os dois discaram na rodada 1 e colidiram. Na 2, só um disca: não há como colidir.
        assert!(discar_nesta_rodada(MAIOR, MENOR, 2));
        assert!(!discar_nesta_rodada(MENOR, MAIOR, 2));
    }

    #[test]
    fn as_rodadas_contam_so_a_reconexao_e_zeram_quando_o_enlace_firma() {
        let mut rodadas = Rodadas::default();
        let reconectar = ConnectMode::Reconnect(MAIOR);
        let vezes: Vec<bool> = (0..4).map(|_| rodadas.discar(MENOR, reconectar)).collect();
        assert_eq!(vezes, vec![true, false, false, true]);
        // Parear não entra na regra, nem conta rodada.
        assert!(rodadas.discar(MENOR, ConnectMode::Pair));
        assert!(!rodadas.discar(MENOR, reconectar), "a quinta rodada");
        rodadas.zerar();
        assert!(
            rodadas.discar(MENOR, reconectar),
            "depois do enlace, é a primeira de novo"
        );
    }
}
