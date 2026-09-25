//! As duas confirmações do pareamento: nada de sessão trafega antes delas.
//!
//! Depois do código de seis dígitos, o enlace espera o usuário daqui **e** o par confirmarem
//! ([04, §3.2](../../../docs/04-seguranca.md)). Uma confirmação só não basta — é o caso que mais
//! fácil se implementa errado, porque parece pronto. A regra é a mesma na rede e no rádio, e por
//! isso mora aqui, sem E/S: o endpoint recebe o passo e o executa.

use super::fio::Kind;

/// O que o endpoint faz depois de uma confirmação.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Desfecho {
    /// Falta a outra confirmação: o enlace continua esperando.
    Esperar,
    /// As duas confirmaram: o enlace vale, e a sessão pode trafegar.
    Promover,
    /// O usuário daqui recusou: o enlace cai.
    Derrubar,
}

/// Quem já confirmou o código.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Confirmacao {
    local_ok: bool,
    peer_ok: bool,
}

impl Confirmacao {
    /// O usuário daqui respondeu à comparação.
    ///
    /// Devolve a espécie a mandar ao par — a resposta vai **sempre**, a recusa também, para o outro
    /// lado sair da tela de comparação em vez de esperar o próprio prazo — e o que fazer em
    /// seguida.
    pub fn local(&mut self, conferiu: bool) -> (Kind, Desfecho) {
        if !conferiu {
            return (Kind::PairReject, Desfecho::Derrubar);
        }
        self.local_ok = true;
        (Kind::PairConfirm, self.desfecho())
    }

    /// O par mandou a confirmação dele.
    pub fn do_par(&mut self) -> Desfecho {
        self.peer_ok = true;
        self.desfecho()
    }

    const fn desfecho(self) -> Desfecho {
        if self.local_ok && self.peer_ok {
            Desfecho::Promover
        } else {
            Desfecho::Esperar
        }
    }
}
