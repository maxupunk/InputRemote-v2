//! O que a interface precisa de um serviço — e nada além disso.
//!
//! A interface fala com um `dyn Servico`, não com um socket. A razão é prática antes de ser
//! doutrinária: o transporte de IPC ainda não existe, e sem esta inversão a janela só poderia
//! ser olhada depois de o serviço estar pronto. Com ela, a interface é desenvolvida, avaliada e
//! testada hoje, contra [`crate::simulado::ServicoSimulado`], e o dia em que o transporte real
//! entrar nada na interface muda.
//!
//! O contrato é pequeno de propósito. Se ele crescer, é sinal de que lógica de produto está
//! vazando para cá — que é exatamente como o v1 acabou com 10.491 linhas no crate da interface.

use ir_ipc::{Autoridade, Aviso, Pedido, Resposta};

/// Um serviço com que a interface consegue conversar.
pub trait Servico {
    /// Faz um pedido e espera a resposta.
    ///
    /// Síncrono porque a interface roda numa thread só e todo pedido é local e curto. Um pedido
    /// que precise demorar — descoberta, pareamento — responde [`Resposta::Feito`] na hora e
    /// conta o resultado depois por [`Servico::avisos`].
    fn pedir(&self, pedido: Pedido) -> Resposta;

    /// Recolhe o que o serviço tem a contar desde a última consulta.
    ///
    /// A interface consulta em intervalo fixo. Fila, e não retorno único, porque perder um aviso
    /// deixaria a tela mostrando um estado que já passou.
    fn avisos(&self) -> Vec<Aviso>;

    /// O privilégio que este processo tem.
    ///
    /// A interface usa para avisar **antes** que uma ação vai ser recusada, em vez de deixar o
    /// usuário descobrir depois de comparar seis dígitos.
    fn autoridade(&self) -> Autoridade;

    /// Se este serviço é o simulado.
    ///
    /// A janela avisa quando for. Uma interface que finge estar funcionando é pior que uma que
    /// não abre.
    fn simulado(&self) -> bool;
}
