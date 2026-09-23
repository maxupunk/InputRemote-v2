//! O desktop protegido e os atalhos de quem tem o teclado.
//!
//! Três coisas que só existem a partir da versão 4:
//!
//! - **Ctrl+Alt+Del.** O de verdade nunca chega a quem tem o teclado — o Windows o intercepta
//!   antes de qualquer gancho —, então quem controla pede com **Ctrl+Alt+End** (o mesmo atalho da
//!   área de trabalho remota) e o par o gera pelo caminho do sistema.
//! - **A recusa no desktop protegido.** O controlado conta quando está descartando digitação na
//!   tela de bloqueio ou no UAC por falta de permissão, e quem digita vê o porquê.
//! - **O atalho de emergência, Ctrl+Alt+Shift+Esc.** Devolve o controle e solta tudo, mesmo com o
//!   enlace saudável. A lógica existia e estava testada; faltava quem a disparasse.
//!
//! A tecla que dispara um atalho não vai ao par — nem a descida, nem a subida. Os modificadores
//! que já desceram lá voltam com o `ReleaseAll` da emergência, ou sobem quando a pessoa os soltar.

use ir_proto::input::{HidUsage, Modifiers};
use ir_proto::message::{Control, Message};
use ir_proto::version::PROTECTED_DESKTOP;

use crate::event::{Command, CommandBatch, Notice};
use crate::phase::Phase;
use crate::session::Session;
use crate::time::Timestamp;

/// A tecla End, do Ctrl+Alt+End que pede o Ctrl+Alt+Del do outro lado.
pub(super) const END: HidUsage = HidUsage(0x4D);
/// A tecla Esc, do Ctrl+Alt+Shift+Esc de emergência.
pub(super) const ESCAPE: HidUsage = HidUsage(0x29);
/// A barra de espaço, do Ctrl+Alt+Shift+Espaço que troca de computador.
pub(super) const SPACE: HidUsage = HidUsage(0x2C);

/// Um atalho de quem tem o teclado.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Shortcut {
    /// Ctrl+Alt+End: Ctrl+Alt+Del no par.
    SecureAttention,
    /// Ctrl+Alt+Shift+Esc: devolver o controle e soltar tudo.
    Emergency,
    /// Ctrl+Alt+Shift+Espaço: levar o controle ao outro computador, ou trazê-lo de volta.
    Switch,
}

/// Qual atalho esta tecla, com estes modificadores já apertados, forma — se forma algum.
#[must_use]
pub(super) fn shortcut(held: Modifiers, usage: HidUsage) -> Option<Shortcut> {
    let any = |left, right| held.contains(left) || held.contains(right);
    let ctrl = any(Modifiers::LEFT_CTRL, Modifiers::RIGHT_CTRL);
    let alt = any(Modifiers::LEFT_ALT, Modifiers::RIGHT_ALT);
    let shift = any(Modifiers::LEFT_SHIFT, Modifiers::RIGHT_SHIFT);
    match usage {
        END if ctrl && alt && !shift => Some(Shortcut::SecureAttention),
        ESCAPE if ctrl && alt && shift => Some(Shortcut::Emergency),
        SPACE if ctrl && alt && shift => Some(Shortcut::Switch),
        _ => None,
    }
}

impl Session {
    /// Se o par entende as mensagens da versão 4.
    fn peer_speaks_protected_desktop(&self) -> bool {
        self.phase.is_established()
            && self
                .peer
                .as_ref()
                .is_some_and(|peer| peer.version >= PROTECTED_DESKTOP)
    }

    /// Uma tecla local que pode ser atalho. Devolve `true` se ela foi consumida aqui.
    ///
    /// Só enquanto o controle está no par: com ele aqui, o teclado é desta máquina, e o Ctrl+Alt+End
    /// é de quem estiver na janela em foco.
    pub(super) fn take_shortcut(
        &mut self,
        now: Timestamp,
        usage: HidUsage,
        pressed: bool,
        out: &mut CommandBatch,
    ) -> bool {
        if !pressed {
            // A subida da tecla que disparou o atalho também não vai.
            return self.swallowed.take_if(|tecla| *tecla == usage).is_some();
        }
        let Some(atalho) = shortcut(self.held_here, usage) else {
            return false;
        };
        let engaged = self.phase == Phase::Engaged;
        match atalho {
            // Com o controle aqui, Ctrl+Alt+End e Ctrl+Alt+Shift+Esc são de quem está em foco.
            Shortcut::SecureAttention | Shortcut::Emergency if !engaged => return false,
            Shortcut::SecureAttention => self.request_secure_attention(now, out),
            // Voltar pelo atalho é voltar soltando tudo lá: os modificadores do atalho desceram no
            // par, e a subida deles vai acontecer aqui.
            Shortcut::Emergency | Shortcut::Switch if engaged => self.on_emergency(now, out),
            Shortcut::Switch | Shortcut::Emergency => self.switch_to_peer(now, out),
        }
        self.swallowed = Some(usage);
        true
    }

    /// Pede ao par que gere Ctrl+Alt+Del.
    pub(super) fn request_secure_attention(&mut self, now: Timestamp, out: &mut CommandBatch) {
        if self.peer_speaks_protected_desktop() {
            self.send(now, Message::Control(Control::SecureAttention), out);
        } else {
            out.push(Command::Notify(Notice::PeerCannotSecureAttention));
        }
    }

    /// A tela daqui bloqueou: pede ao par que bloqueie a dele.
    pub(super) fn request_peer_lock(&mut self, now: Timestamp, out: &mut CommandBatch) {
        if self.peer_speaks_protected_desktop() {
            self.send(now, Message::Control(Control::LockScreen), out);
        }
    }

    /// A periferia daqui passou a recusar, ou voltou a aceitar, digitação no desktop protegido.
    pub(super) fn on_local_protected_desktop(
        &mut self,
        now: Timestamp,
        refused: bool,
        out: &mut CommandBatch,
    ) {
        if self.peer_speaks_protected_desktop() {
            self.send(
                now,
                Message::Control(Control::ProtectedDesktop { refused }),
                out,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ctrl_alt_end_asks_for_secure_attention_from_either_hand() {
        let left = Modifiers::LEFT_CTRL.union(Modifiers::LEFT_ALT);
        let right = Modifiers::RIGHT_CTRL.union(Modifiers::RIGHT_ALT);
        assert_eq!(shortcut(left, END), Some(Shortcut::SecureAttention));
        assert_eq!(shortcut(right, END), Some(Shortcut::SecureAttention));
        assert_eq!(
            shortcut(Modifiers::LEFT_CTRL, END),
            None,
            "Ctrl+End é do editor"
        );
    }

    #[test]
    fn ctrl_alt_shift_esc_is_the_emergency() {
        let held = Modifiers::LEFT_CTRL
            .union(Modifiers::LEFT_ALT)
            .union(Modifiers::LEFT_SHIFT);
        assert_eq!(shortcut(held, ESCAPE), Some(Shortcut::Emergency));
        assert_eq!(
            shortcut(Modifiers::LEFT_CTRL.union(Modifiers::LEFT_SHIFT), ESCAPE),
            None,
            "Ctrl+Shift+Esc é o gerenciador de tarefas do outro lado"
        );
    }
}
