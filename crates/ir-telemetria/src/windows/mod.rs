//! As fontes do Windows.

mod sistema;
mod termica;

use crate::fonte::{Agendada, cadencia};

/// As fontes desta máquina Windows, cada uma na sua cadência.
pub(crate) fn fontes() -> Vec<Agendada> {
    vec![
        Agendada::nova(sistema::Processador::default(), cadencia::RAPIDA),
        Agendada::nova(sistema::Memoria, cadencia::RAPIDA),
        Agendada::nova(sistema::Rede::default(), cadencia::RAPIDA),
        Agendada::nova(sistema::Bateria, cadencia::MEDIA),
        Agendada::nova(termica::Termica::nova(), cadencia::MEDIA),
        Agendada::nova(sistema::Disco::do_sistema(), cadencia::LENTA),
    ]
}
