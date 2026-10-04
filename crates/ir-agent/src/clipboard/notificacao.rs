//! A notificação do sistema, no Linux.
//!
//! No Windows a interface fica na bandeja e mostra a notificação nativa. No GNOME não há bandeja, e
//! a janela costuma estar fechada — quem copia está no Nautilus. O lugar certo do recado ali é a
//! notificação do sistema, e quem sempre está de pé na sessão é este ajudante.
//!
//! O que dizer e quando é o mesmo dos dois sistemas, e mora em [`ir_recado`]: aqui só se liga
//! o ritmo das cópias ao mostrador do Linux.

use std::time::{Duration, Instant};

use ir_ipc::transferencia::Transferencia;
use ir_recado::{Recado, Ritmo};

/// De quanto em quanto tempo o andamento vai para a tela.
///
/// Dois segundos: o bastante para ver o número mudar, e pouco para o banner não virar estroboscópio.
const INTERVALO: Duration = Duration::from_secs(2);

/// Conta ao usuário as cópias e o que mais acontece sem a janela aberta.
#[derive(Debug)]
pub(crate) struct Notificador {
    ritmo: Ritmo,
    #[cfg(target_os = "linux")]
    mostrador: ir_recado::linux::NotifySend,
}

impl Default for Notificador {
    fn default() -> Self {
        Self {
            ritmo: Ritmo::novo(INTERVALO),
            #[cfg(target_os = "linux")]
            mostrador: ir_recado::linux::NotifySend::default(),
        }
    }
}

impl Notificador {
    /// Conta esta cópia, se for hora.
    pub(crate) fn contar(&mut self, copia: &Transferencia) {
        if let Some(vez) = self.ritmo.vez(copia, Instant::now()) {
            self.mostrar(&Recado::da_copia(copia), !vez.primeira);
        }
    }

    /// Conta um recado avulso.
    pub(crate) fn avisar(&mut self, recado: &Recado) {
        self.mostrar(recado, false);
    }

    #[cfg(target_os = "linux")]
    fn mostrar(&mut self, recado: &Recado, substituir: bool) {
        self.mostrador.mostrar(recado, substituir);
    }

    /// Fora do Linux quem conta é a interface, com a notificação nativa.
    #[cfg(not(target_os = "linux"))]
    #[allow(clippy::unused_self)]
    const fn mostrar(&mut self, _recado: &Recado, _substituir: bool) {}
}
