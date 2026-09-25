//! As duas confirmações do pareamento: nada de sessão trafega antes delas.
//!
//! Depois do código de seis dígitos, o enlace fica em `AguardandoConfirmacao` até o usuário daqui
//! **e** o par confirmarem ([04, §3.2](../../../../docs/04-seguranca.md)). Uma confirmação só não
//! basta — é o caso que mais fácil se implementa errado, porque parece pronto. A regra é a
//! [`Confirmacao`](ir_crypto::enlace::Confirmacao) do `ir-crypto`, a mesma da rede; aqui ela só é
//! executada.

use ir_crypto::enlace::Desfecho;

use super::{BtEvent, Endpoint, Estado};
use crate::radio::Radio;

impl<R: Radio> Endpoint<R> {
    /// O usuário respondeu à comparação de códigos.
    ///
    /// A resposta vai ao par nos dois casos, e uma falha ao mandá-la é contada.
    pub(super) async fn confirmar(&mut self, ok: bool) {
        let (enviado, desfecho) = {
            let Estado::AguardandoConfirmacao {
                enlace,
                confirmacao,
                ..
            } = &mut self.estado
            else {
                return;
            };
            let (especie, desfecho) = confirmacao.local(ok);
            (enlace.enviar(especie, &[]).await, desfecho)
        };
        if let Err(erro) = enviado {
            self.relatar(&erro);
        }
        self.seguir(desfecho);
    }

    /// O par mandou a confirmação dele.
    pub(super) fn par_confirmou(&mut self) {
        if let Estado::AguardandoConfirmacao { confirmacao, .. } = &mut self.estado {
            let desfecho = confirmacao.do_par();
            self.seguir(desfecho);
        }
    }

    /// Executa o que as confirmações decidiram.
    fn seguir(&mut self, desfecho: Desfecho) {
        match desfecho {
            Desfecho::Esperar => {}
            Desfecho::Derrubar => self.derrubar("códigos diferentes"),
            Desfecho::Promover => {
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
    }
}
