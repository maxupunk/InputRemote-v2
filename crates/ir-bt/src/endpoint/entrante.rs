//! Quem liga para esta máquina, e o fim de todo handshake: pareamento ou reconexão.
//!
//! Saiu de [`super`] por tamanho. É também onde mora a regra de que um pedido de pareamento que
//! chega de fora só é atendido quando o serviço deixa ([`BtCommand::AcceptPairing`]).

#[cfg(doc)]
use super::BtCommand;
use ir_crypto::enlace::Confirmacao;

use super::{BtEvent, Endpoint, Estado, FALHAS_ATE_PERDER_O_RADIO};
use crate::addr::BdAddr;
use crate::canal::Quadros;
use crate::error::{BtError, Result};
use crate::handshake;
use crate::link::EnlaceSeguro;
use crate::radio::Radio;
use crate::wire::Mode;

impl<R: Radio> Endpoint<R> {
    /// Alguém ligou para esta máquina: responde ao handshake. Devolve se o laço continua.
    ///
    /// A escuta que falha seguidas vezes — ou que diz que o rádio sumiu — é um rádio perdido: o
    /// endpoint conta isso e termina, e quem o subiu tenta abrir o rádio de novo em segundo plano.
    pub(super) async fn receber_entrante(&mut self, entrante: Result<(R::Canal, BdAddr)>) -> bool {
        let (canal, peer) = match entrante {
            Ok(entrante) => {
                self.falhas_da_escuta = 0;
                entrante
            }
            Err(erro) => {
                self.falhas_da_escuta = self.falhas_da_escuta.saturating_add(1);
                let perdido = matches!(erro, BtError::SemRadio(_))
                    || self.falhas_da_escuta >= FALHAS_ATE_PERDER_O_RADIO;
                if perdido {
                    self.contar(BtEvent::RadioLost(erro.to_string()));
                    return false;
                }
                self.relatar(&erro);
                // Um erro solto de `accept` acontece; um laço deles sem pausa não pode acontecer.
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                return true;
            }
        };
        let mut quadros = Quadros::novo(canal);
        let (modo, mensagem) = match handshake::esperar_inicio(&mut quadros).await {
            Ok(inicio) => inicio,
            Err(erro) => {
                self.relatar(&erro);
                return true;
            }
        };
        if modo == Mode::Pair && !self.aceitar_pareamento {
            // Antes de qualquer criptografia, como na rede: recusar custa um byte lido, e quem
            // ligou não chega a ver código nenhum. O canal cai junto com `quadros`.
            tracing::warn!(%peer, "pedido de pareamento recusado: há par, e a janela de pareamento não está aberta");
            return true;
        }
        match handshake::responder(&mut quadros, &self.identity, modo, &mensagem).await {
            Ok(pronto) => self.estabelecer(quadros, peer, pronto),
            Err(erro) => self.relatar(&erro),
        }
        true
    }

    /// Um handshake terminou: ou pede confirmação (pareamento), ou já estabelece (reconexão).
    pub(super) fn estabelecer(
        &mut self,
        quadros: Quadros<R::Canal>,
        peer: BdAddr,
        pronto: handshake::Established,
    ) {
        let enlace = EnlaceSeguro::novo(quadros, pronto.transport);
        let peer_static = pronto.peer_static;
        self.rodadas.zerar();
        if let Some(code) = pronto.code {
            // Pareamento: mostra o código e espera as duas confirmações antes de deixar qualquer
            // quadro de sessão passar.
            self.contar(BtEvent::PairingCode {
                code,
                peer_static,
                peer,
            });
            self.estado = Estado::AguardandoConfirmacao {
                enlace,
                peer_static,
                peer,
                confirmacao: Confirmacao::default(),
            };
        } else {
            // Reconexão: a identidade já está fixada, então o enlace já vale.
            self.contar(BtEvent::Established { peer_static, peer });
            self.estado = Estado::Estabelecido { enlace };
        }
    }
}
