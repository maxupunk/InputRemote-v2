//! O que a sessão pede, e o buffer que carrega os pedidos.

use ir_proto::carrier::Carrier;
use ir_proto::frame::Frame;
use ir_proto::input::{Button, HidUsage, PointerPosition, WheelDelta};

use super::Notice;
use crate::time::Timestamp;

/// Uma entrada a injetar na máquina local. Só o cliente recebe estes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Injection {
    /// Uma tecla física.
    Key {
        /// Qual.
        usage: HidUsage,
        /// `true` para pressionar.
        pressed: bool,
    },
    /// Um botão do ponteiro.
    Button {
        /// Qual.
        button: Button,
        /// `true` para pressionar.
        pressed: bool,
    },
    /// Movimento de roda.
    Wheel(WheelDelta),
    /// Onde o ponteiro deve estar.
    ///
    /// Sempre absoluto, nunca relativo — `docs/05-windows.md` §4.2: injetar movimento
    /// relativo faria o sistema aplicar a própria aceleração a deltas que já vêm acelerados.
    Pointer(PointerPosition),
}

/// Um prazo que a sessão pediu para ser acordada.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum TimerId {
    /// Hora de mandar `Ping`.
    Heartbeat,
    /// Hora de declarar o enlace caído.
    LinkTimeout,
    /// Hora de mandar o estado completo.
    Snapshot,
    /// Hora de despachar o movimento de ponteiro acumulado.
    PointerFlush,
    /// Hora de tentar reconectar.
    Reconnect,
}

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

    /// Acorde a sessão neste instante.
    ///
    /// Absoluto e não relativo: a periferia não precisa saber quando o pedido foi feito, e um
    /// atraso na fila não desloca o prazo.
    SetTimer {
        /// Qual prazo.
        id: TimerId,
        /// Quando.
        at: Timestamp,
    },

    /// Cancele este prazo.
    ClearTimer(TimerId),

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
