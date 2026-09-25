//! O que a sessão pede, e o buffer que carrega os pedidos.

use ir_proto::carrier::Carrier;
use ir_proto::frame::Frame;
use ir_proto::input::PointerPosition;

use super::Notice;

/// Uma entrada a injetar na máquina local. Só o cliente recebe estes.
///
/// O tipo é o de `ir-proto`, o mesmo que o injetor e o canal do agente usam: a injeção atravessa
/// o serviço e o agente sem tradução nenhuma.
pub use ir_proto::input::Injection;

/// O que a sessão pede.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Command {
    /// Mande este quadro por este portador.
    Send {
        /// Por onde.
        carrier: Carrier,
        /// O quê.
        frame: Frame,
    },

    /// Injete esta entrada na máquina local.
    Inject(Injection),

    /// Solte tudo que estiver pressionado, agora.
    ///
    /// O comando mais importante do produto. Emitido em toda falha, em toda queda e em todo
    /// encerramento (`docs/02-arquitetura.md` §8).
    ReleaseAll,

    /// Ligue ou desligue a supressão da entrada local.
    ///
    /// Ligada enquanto o controle está no par: o teclado e o mouse desta máquina param de
    /// afetá-la e passam a alimentar só a sessão.
    SuppressLocalInput(bool),

    /// Ponha o ponteiro local aqui.
    ///
    /// Usado ao devolver o controle, para o cursor reaparecer na borda por onde voltou.
    WarpPointer(PointerPosition),

    /// Gere Ctrl+Alt+Del nesta máquina, se o administrador daqui permitir. Pedido pelo par.
    SecureAttention,

    /// Bloqueie a tela desta máquina. Pedido pelo par, que bloqueou a dele.
    LockScreen,

    /// Conte isto à interface.
    Notify(Notice),

    /// Chegou texto do par, inteiro e conferido: ponha-o no clipboard desta máquina.
    ClipboardText(super::ClipText),
}

/// Os comandos produzidos por um passo.
///
/// É um buffer que **quem chama possui e reaproveita**, e não um `Vec` novo por evento. O
/// caminho quente processa milhares de eventos por segundo, e a regra 2 de
/// `docs/02-arquitetura.md` §6 é não alocar ali.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct CommandBatch {
    commands: Vec<Command>,
}

impl CommandBatch {
    /// Um lote vazio.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            commands: Vec::new(),
        }
    }

    /// Um lote vazio com espaço já reservado.
    #[must_use]
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            commands: Vec::with_capacity(capacity),
        }
    }

    /// Esvazia sem devolver a memória, para o próximo passo reaproveitá-la.
    pub fn clear(&mut self) {
        self.commands.clear();
    }

    /// Acrescenta um comando.
    pub fn push(&mut self, command: Command) {
        self.commands.push(command);
    }

    /// Os comandos, na ordem em que foram produzidos.
    ///
    /// A ordem importa: `ReleaseAll` sempre vem antes de qualquer coisa que possa falhar.
    #[must_use]
    pub fn as_slice(&self) -> &[Command] {
        &self.commands
    }

    /// Quantos comandos há.
    #[must_use]
    pub fn len(&self) -> usize {
        self.commands.len()
    }

    /// Se nenhum comando foi produzido.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.commands.is_empty()
    }

    /// Itera os comandos.
    pub fn iter(&self) -> impl Iterator<Item = &Command> {
        self.commands.iter()
    }

    /// Tira os comandos na ordem, por valor, e deixa o lote vazio com a capacidade que tinha.
    pub fn drain(&mut self) -> impl Iterator<Item = Command> + '_ {
        self.commands.drain(..)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_cleared_batch_keeps_its_capacity() {
        let mut batch = CommandBatch::with_capacity(16);
        batch.push(Command::ReleaseAll);
        assert_eq!(batch.len(), 1);
        batch.clear();
        assert!(batch.is_empty(), "esvaziou");
        // A capacidade reservada continua lá; é o que evita alocar por evento.
        batch.push(Command::ReleaseAll);
        assert_eq!(batch.len(), 1);
    }

    #[test]
    fn the_batch_preserves_order() {
        let mut batch = CommandBatch::new();
        batch.push(Command::ReleaseAll);
        batch.push(Command::SuppressLocalInput(false));
        assert_eq!(
            batch.as_slice(),
            &[Command::ReleaseAll, Command::SuppressLocalInput(false)],
            "ReleaseAll vem primeiro, e a ordem é contrato"
        );
    }
}
