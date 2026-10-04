//! "Copiar e colar": uma escolha só, valendo para os dois computadores.
//!
//! Desligado num lado e ligado no outro, o Ctrl+C de lá continuava saindo para cá só para ser
//! recusado — e quem tinha desligado via, no próprio computador, "a cópia não atravessou". A
//! pessoa desliga para que **nada** atravesse; o certo é o outro lado parar junto.
//!
//! Mesma conversa da borda (`edge`): cada ponta anuncia a escolha dela com o horário em que foi
//! feita ([`Control::CopyPaste`]), ao estabelecer e a cada troca. Se divergirem, vale a mais
//! recente. No empate — duas escolhas sem horário, de antes desta versão —, vale **desligado**: é
//! o lado que alguém escolheu de propósito, já que o padrão é ligado. As duas pontas fazem a mesma
//! conta com os valores trocados, então exatamente uma cede.
//!
//! A mensagem é da versão 9: um par mais antigo não a recebe, e cada lado segue com a sua.

use ir_proto::message::{Control, Message};
use ir_proto::version;

use crate::config::CopyPaste;
use crate::event::{Command, CommandBatch, Notice};
use crate::session::Session;
use crate::time::Timestamp;

impl Session {
    /// O usuário ligou ou desligou, aqui.
    pub(super) fn on_set_copy_paste(
        &mut self,
        now: Timestamp,
        choice: CopyPaste,
        out: &mut CommandBatch,
    ) {
        if choice == self.config.copy_paste {
            return;
        }
        self.config.copy_paste = choice;
        self.announce_copy_paste(now, out);
    }

    /// Conta ao par a escolha daqui, se há sessão e o par entende.
    pub(super) fn announce_copy_paste(&mut self, now: Timestamp, out: &mut CommandBatch) {
        let entende = self
            .peer
            .as_ref()
            .is_some_and(|peer| version::supports_copy_paste(peer.version));
        if !self.phase.is_established() || !entende {
            return;
        }
        let CopyPaste { enabled, chosen_at } = self.config.copy_paste;
        self.send(
            now,
            Message::Control(Control::CopyPaste { enabled, chosen_at }),
            out,
        );
    }

    /// O par contou a escolha dele. Se divergir da daqui e for a que vale, passa a valer aqui.
    pub(super) fn on_copy_paste(&mut self, theirs: CopyPaste, out: &mut CommandBatch) {
        let mine = self.config.copy_paste;
        if theirs.enabled == mine.enabled || !prevails(theirs, mine) {
            return; // o par faz a mesma conta com o anúncio daqui, e cede
        }
        // O horário do par, e não o de agora: senão esta ponta venceria a próxima comparação.
        self.config.copy_paste = theirs;
        out.push(Command::Notify(Notice::CopyPasteAdopted(theirs)));
    }
}

/// Se a escolha `a` vale sobre a `b`: a mais recente; no empate, a que desliga.
fn prevails(a: CopyPaste, b: CopyPaste) -> bool {
    a.chosen_at > b.chosen_at || (a.chosen_at == b.chosen_at && !a.enabled && b.enabled)
}

#[cfg(test)]
mod tests {
    use super::*;

    const fn escolha(enabled: bool, chosen_at: u64) -> CopyPaste {
        CopyPaste { enabled, chosen_at }
    }

    #[test]
    fn de_duas_escolhas_diferentes_exatamente_uma_vale() {
        for (ta, tb) in [(0, 0), (5, 3), (3, 5), (7, 7)] {
            let (a, b) = (escolha(true, ta), escolha(false, tb));
            assert_ne!(prevails(a, b), prevails(b, a), "horários {ta} e {tb}");
        }
    }

    #[test]
    fn vale_a_mais_recente_mesmo_que_ligue() {
        assert!(prevails(escolha(true, 10), escolha(false, 5)));
        assert!(!prevails(escolha(false, 5), escolha(true, 10)));
    }

    /// O caso de quem atualiza com a opção já desligada num lado: nenhum dos dois tem horário, e
    /// o desligado é o que alguém escolheu.
    #[test]
    fn no_empate_vale_desligado() {
        assert!(prevails(escolha(false, 0), escolha(true, 0)));
        assert!(!prevails(escolha(true, 0), escolha(false, 0)));
    }
}
