//! Quando um pedido de pareamento que chega de fora é atendido.
//!
//! Sem par gravado, sempre: é a primeira experiência com o produto, e o outro computador aparece na
//! frente sozinho com o código. Com par gravado, só enquanto a janela de pareamento estiver aberta
//! aqui — procurar ou começar um pareamento abre, por alguns minutos. Antes qualquer um na rede
//! local podia mandar um pedido a cada poucos segundos: o transporte ficava esperando uma
//! confirmação que nunca vinha, a reconexão ao par de verdade não era atendida, e um código
//! aparecia na tela sem ninguém ter pedido (log 45).
//!
//! Quem recusa é o transporte, antes de qualquer criptografia; aqui mora só a decisão.

use std::time::{Duration, Instant};

use tracing::info;

use super::Daemon;

/// Por quanto tempo procurar ou começar um pareamento deixa um pedido de fora entrar.
pub(super) const ABERTA_POR: Duration = Duration::from_secs(180);

impl Daemon {
    /// A pessoa abriu a janela de pareamento aqui: pedidos de fora entram por um tempo.
    pub(super) fn abrir_para_pareamento(&mut self) {
        self.pareamento_aberto_ate = Some(Instant::now() + ABERTA_POR);
        self.anunciar_abertura();
    }

    /// Se um pedido de pareamento de fora seria atendido agora.
    pub(super) fn aceita_pareamento_de_fora(&self, agora: Instant) -> bool {
        self.config.peers.is_empty()
            || self.pareamento.is_some()
            || self.pareamento_aberto_ate.is_some_and(|ate| agora < ate)
    }

    /// Conta aos transportes a decisão de agora, quando ela muda.
    pub(crate) fn anunciar_abertura(&mut self) {
        let aceita = self.aceita_pareamento_de_fora(Instant::now());
        if self.abertura_anunciada == Some(aceita) {
            return;
        }
        self.abertura_anunciada = Some(aceita);
        info!(aceita, "pedidos de pareamento de fora");
        self.rede.aceitar_pareamento(aceita);
        if let Some(radio) = self.radio.as_ref() {
            radio.aceitar_pareamento(aceita);
        }
    }
}

#[cfg(test)]
mod tests {

    use super::*;
    use crate::actor::bancada::{Bancada, Feito};
    use crate::config::PinnedPeer;

    fn com_par(bancada: &mut Bancada) {
        bancada.daemon.config.peers = vec![PinnedPeer {
            pubkey: "00".repeat(32),
            addr: Some("10.0.0.2:52525".to_owned()),
            radio: None,
            nome: None,
            tela_de_bloqueio: false,
        }];
    }

    #[test]
    fn sem_par_gravado_o_pareamento_de_fora_entra() {
        let mut bancada = Bancada::nova();
        bancada.daemon.anunciar_abertura();
        assert!(
            bancada
                .rede
                .feitos()
                .contains(&Feito::AceitaPareamento(true))
        );
    }

    #[test]
    fn com_par_gravado_so_entra_com_a_janela_de_pareamento_aberta() {
        let mut bancada = Bancada::nova();
        com_par(&mut bancada);
        bancada.daemon.anunciar_abertura();
        assert_eq!(bancada.rede.feitos(), vec![Feito::AceitaPareamento(false)]);
        assert_eq!(bancada.radio.feitos(), vec![Feito::AceitaPareamento(false)]);

        bancada.daemon.abrir_para_pareamento();
        assert_eq!(bancada.rede.feitos(), vec![Feito::AceitaPareamento(true)]);

        let depois = Instant::now() + ABERTA_POR + Duration::from_secs(1);
        assert!(
            !bancada.daemon.aceita_pareamento_de_fora(depois),
            "fecha sozinha"
        );
    }

    #[test]
    fn a_decisao_so_e_repetida_quando_muda() {
        let mut bancada = Bancada::nova();
        bancada.daemon.anunciar_abertura();
        let _ = bancada.rede.feitos();
        bancada.daemon.anunciar_abertura();
        assert!(bancada.rede.feitos().is_empty());
    }
}
