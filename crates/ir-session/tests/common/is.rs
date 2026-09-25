//! Testes prontos, para as asserções ficarem legíveis nos cenários.

use ir_proto::message::{Control, Message};
use ir_session::Command;

pub fn release_all(command: &Command) -> bool {
    matches!(command, Command::ReleaseAll)
}

pub fn injection(command: &Command) -> bool {
    matches!(command, Command::Inject(_))
}

pub fn suppress(command: &Command) -> bool {
    matches!(command, Command::SuppressLocalInput(true))
}

pub fn unsuppress(command: &Command) -> bool {
    matches!(command, Command::SuppressLocalInput(false))
}

pub fn warp(command: &Command) -> bool {
    matches!(command, Command::WarpPointer(_))
}

pub fn enter_screen(command: &Command) -> bool {
    matches!(
        command,
        Command::Send { frame, .. }
            if matches!(frame.message, Message::Control(Control::EnterScreen { .. }))
    )
}

pub fn snapshot(command: &Command) -> bool {
    matches!(
        command,
        Command::Send { frame, .. }
            if matches!(frame.message, Message::Control(Control::StateSnapshot { .. }))
    )
}

pub fn leave_screen(command: &Command) -> bool {
    matches!(
        command,
        Command::Send { frame, .. }
            if matches!(frame.message, Message::Control(Control::LeaveScreen { .. }))
    )
}
