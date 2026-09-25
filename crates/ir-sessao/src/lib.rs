//! Quem nasce **dentro** da sessão do usuário, e quem cuida para que continue lá.
//!
//! O serviço roda na sessão 0 do Windows, onde não há teclado, mouse nem clipboard de ninguém.
//! Dois processos do produto precisam nascer na sessão interativa: o **agente**, que captura e
//! injeta entrada, e o **ajudante de clipboard**, que lê e escreve o clipboard do usuário. Os dois
//! nascem de um serviço, e essa passagem — duplicar token, mover para a sessão de console, montar
//! o ambiente certo, escolher o desktop — é uma responsabilidade só, e é esta.
//!
//! Ela virou crate quando o serviço passou do orçamento de linhas de `docs/09` §1. O limite não é
//! um número: é o aviso de que faltava uma fronteira — e esta é a fronteira. O serviço decide
//! *quando* quer um agente ou um ajudante; como isso acontece no Windows não é assunto dele.
//!
//! Fora do Windows o crate compila e não faz nada: no Linux quem sobe o ajudante é o `systemd` do
//! usuário, que sabe quando há sessão gráfica, e quem injeta é o próprio serviço por `uinput`.

mod ajudantes;
#[cfg(windows)]
pub mod atencao;
#[cfg(windows)]
mod lancador;
mod zelador;

pub use ajudantes::{Ajudantes, Presenca};
#[cfg(windows)]
pub use lancador::{
    como_servico, lancar_agente, lancar_ajudante_de_clipboard, marcar_como_servico,
};
pub use zelador::{Zelador, zelar_pelo_clipboard};
