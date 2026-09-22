//! As duas confirmações do pareamento: nada de sessão trafega antes delas.
//!
//! Depois do código de seis dígitos, o enlace fica em `AguardandoConfirmacao` até o usuário daqui
//! **e** o par confirmarem ([04, §3.2](../../../../docs/04-seguranca.md)). Uma confirmação só não
//! basta — é o caso que mais fácil se implementa errado, porque parece pronto.

use crate::wire::Kind;

use super::{BtEvent, Endpoint, Estado};
use crate::radio::Radio;

impl<R: Radio> Endpoint<R> {
    /// O usuário respondeu à comparação de códigos.
    pub(super) async fn confirmar(&mut self, ok: bool) {
        let enviado = {
            let Estado::AguardandoConfirmacao {
                enlace, local_ok, ..
            } = &mut self.estado
            else {
                return;
            };
            if ok {
                *local_ok = true;
            }
            let especie = if ok {
                Kind::PairConfirm
            } else {
                Kind::PairReject
            };
            enlace.enviar(especie, &[]).await
        };
        if let Err(erro) = enviado {
            self.relatar(&erro);
        }
        if ok {
            self.promover_se_pronto();
        } else {
            self.derrubar("códigos diferentes");
        }
    }

    /// Estabelece o enlace quando os dois lados confirmaram.
    pub(super) fn promover_se_pronto(&mut self) {
        if !matches!(
            &self.estado,
            Estado::AguardandoConfirmacao {
                local_ok: true,
                peer_ok: true,
                ..
            }
        ) {
            return;
        }
        let anterior = core::mem::replace(&mut self.estado, Estado::Ocioso);
        if let Estado::AguardandoConfirmacao {
            enlace,
            peer_static,
            peer,
            ..
        } = anterior
        {
            self.contar(BtEvent::Established { peer_static, peer });
            self.estado = Estado::Estabelecido { enlace };
        }
    }
}
