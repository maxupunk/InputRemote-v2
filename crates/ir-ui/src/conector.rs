//! Como a interface abre o canal até o serviço — e só isso.
//!
//! Separado da conexão ([`crate::conexao`]) de propósito: abrir um *named pipe* ou um socket Unix é
//! um detalhe de plataforma, e manter a vida da conexão (perceber a queda, esperar, reconectar)
//! dependente só desta abstração é o que permite testar a reconexão inteira sem serviço, sem
//! *pipe* e sem sistema operacional específico — os testes entregam um conector próprio.

/// As duas metades de um canal duplex: por onde se escreve e por onde se lê.
pub type Duplex = ir_ipc::cliente::Duplex;

/// Quem sabe abrir um canal até o serviço.
pub trait Conector: Send {
    /// Abre um canal novo.
    ///
    /// # Errors
    ///
    /// O erro do sistema ao abrir. `NotFound` e afins significam serviço fora do ar;
    /// `PermissionDenied`, que o canal existe e este usuário não o alcança.
    fn abrir(&self) -> std::io::Result<Duplex>;
}

/// O conector desta plataforma: *named pipe* no Windows, socket Unix no Linux.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConectorLocal {
    endereco: String,
}

impl ConectorLocal {
    /// O conector para o endereço padrão, ou para o de `IR_CONTROL_ENDPOINT` se houver.
    #[must_use]
    pub fn padrao() -> Self {
        Self {
            endereco: endereco_configurado(),
        }
    }

    /// Onde este conector procura o serviço.
    #[must_use]
    pub fn endereco(&self) -> &str {
        &self.endereco
    }
}

impl Default for ConectorLocal {
    fn default() -> Self {
        Self::padrao()
    }
}

impl Conector for ConectorLocal {
    fn abrir(&self) -> std::io::Result<Duplex> {
        abrir_canal(&self.endereco)
    }
}

/// O endereço do canal de controle, pela mesma regra de todo cliente
/// ([`ir_ipc::endereco::do_controle`]).
fn endereco_configurado() -> String {
    ir_ipc::endereco::do_controle()
}

/// Abre a conexão e devolve as duas metades sobre o mesmo canal duplex.
///
/// Pelo cliente compartilhado de `ir-ipc`, e não com `std::fs::File`: no Windows um *named pipe*
/// síncrono trava a escrita enquanto a thread de leitura espera o serviço falar, e era isso que
/// congelava a janela ao abrir e ao clicar em "Parear" (log 21).
fn abrir_canal(endereco: &str) -> std::io::Result<Duplex> {
    ir_ipc::cliente::abrir(endereco)
}
