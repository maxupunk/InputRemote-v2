//! A economia de energia do Wi-Fi: contar ao par como está a placa daqui, e repassar o que ele conta.
//!
//! Com a economia ligada, a placa cochila entre pacotes e os comandos pela rede chegam em rajadas:
//! o ponteiro flui, trava e volta a fluir. Quem sente é quem está olhando a tela — quase sempre o
//! **outro** computador —, então cada ponta anuncia o estado da própria placa, e a tela do par pode
//! mostrar o aviso e o botão que pede para desligar.
//!
//! A sessão não mede nem muda nada: ela anuncia o que a periferia viu e repassa o que ouviu. E só
//! fala disso com um par da versão 3 — um da versão 2 não decodificaria as mensagens.

use ir_proto::message::{Control, Message, NetworkPowerSaving};
use ir_proto::version::NETWORK_POWER;

use crate::event::{Command, CommandBatch, Notice};
use crate::session::Session;
use crate::time::Timestamp;

impl Session {
    /// Se o par entende as mensagens de economia de energia.
    fn peer_speaks_network_power(&self) -> bool {
        self.phase.is_established()
            && self
                .peer
                .as_ref()
                .is_some_and(|peer| peer.version >= NETWORK_POWER)
    }

    /// A periferia viu como está a economia de energia do Wi-Fi daqui.
    ///
    /// Anuncia toda leitura, mudando ou não. A periferia lê a cada 30 s, e repetir custa uma
    /// mensagem de poucos bytes; em troca, o que o par sabe converge sozinho mesmo quando uma
    /// mudança não chegou lá — o que a bancada mostrou acontecer uma vez, sem queda de sessão que o
    /// explicasse (log 44).
    pub(super) fn on_local_network_power(
        &mut self,
        now: Timestamp,
        state: NetworkPowerSaving,
        out: &mut CommandBatch,
    ) {
        self.local_power = Some(state);
        self.announce_network_power(now, out);
    }

    /// Conta ao par como está a placa daqui, se se sabe e se ele entende.
    pub(super) fn announce_network_power(&mut self, now: Timestamp, out: &mut CommandBatch) {
        if let Some(state) = self.local_power
            && self.peer_speaks_network_power()
        {
            self.send(now, Message::Control(Control::NetworkPower(state)), out);
        }
    }

    /// O usuário pediu, daqui, que o par desligue a economia de energia do Wi-Fi dele.
    ///
    /// Devolve se o pedido saiu: sem sessão, ou com um par que não entende, não há como pedir.
    pub(super) fn on_disable_peer_network_power(
        &mut self,
        now: Timestamp,
        out: &mut CommandBatch,
    ) -> bool {
        if !self.peer_speaks_network_power() {
            return false;
        }
        self.send(
            now,
            Message::Control(Control::DisableNetworkPowerSaving),
            out,
        );
        true
    }

    /// O par contou como está a placa dele.
    pub(super) fn on_peer_network_power(state: NetworkPowerSaving, out: &mut CommandBatch) {
        out.push(Command::Notify(Notice::PeerNetworkPower(state)));
    }

    /// O par pediu que a placa daqui não cochile. Quem aplica é a periferia.
    pub(super) fn on_network_power_fix_requested(out: &mut CommandBatch) {
        out.push(Command::Notify(Notice::NetworkPowerFixRequested));
    }
}
