//! O gancho de suspensão do `systemd`, ouvido por sinais.
//!
//! O gancho em `system-sleep` manda `SIGUSR1` antes de dormir e `SIGUSR2` ao acordar
//! (`empacotar/linux/inputremote-sleep`). Sinal, e não D-Bus: o gancho já roda como root na hora
//! certa, e o serviço não precisa de uma dependência nova para ouvir uma coisa só.

use tokio::signal::unix::{SignalKind, signal};
use tokio::sync::mpsc::UnboundedSender;

use crate::EventoDoSistema;

/// Ouve os dois sinais e os conta como [`EventoDoSistema`]. Precisa de uma runtime `tokio`.
pub fn ouvir_o_gancho_de_suspensao(destino: UnboundedSender<EventoDoSistema>) {
    let (Ok(mut dormir), Ok(mut acordar)) = (
        signal(SignalKind::user_defined1()),
        signal(SignalKind::user_defined2()),
    ) else {
        tracing::warn!("sem os sinais do gancho de suspensão; a suspensão não será anunciada");
        return;
    };
    tokio::spawn(async move {
        loop {
            let evento = tokio::select! {
                _ = dormir.recv() => EventoDoSistema::Suspendendo,
                _ = acordar.recv() => EventoDoSistema::Retomou,
            };
            if destino.send(evento).is_err() {
                return;
            }
        }
    });
}
