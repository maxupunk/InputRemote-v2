//! A amostra de uso para o painel no Android, e a latência de cada rota.
//!
//! O computador que tem um painel olhando pede ao par a amostra dele ([`Control::WatchUsage`]), e o
//! par passa a mandar uma por segundo ([`Control::Usage`]) até o pedido ser desfeito. Quem coleta e
//! quem decide o ritmo é a periferia: a sessão só leva o pedido e a amostra, e só a um par da
//! versão 11 ([12](../../../../docs/12-painel-android.md)).
//!
//! A latência por rota vem do `Pong` que já existe. Na rota dupla o `Pong` chega pelos dois
//! portadores; só o primeiro passa da detecção de repetição, então a medida de cada portador é
//! tirada **antes** dela ([`Session::measure_route`]). O par responde ao primeiro `Ping` que lhe
//! chega, então a medida do portador mais lento é a ida pelo mais rápido somada à volta por ele: é
//! uma estimativa honesta de "quanto esta rota atrasa", e exata quando a rota tem um portador só.

use ir_proto::carrier::Carrier;
use ir_proto::frame::Frame;
use ir_proto::message::{Control, Message, UsageSample};
use ir_proto::version;

use crate::event::{Command, CommandBatch, Notice};
use crate::session::Session;
use crate::time::Timestamp;

impl Session {
    /// Se o par entende o pedido de amostras: há sessão, e ele é da versão 11.
    ///
    /// Exposta para o serviço mostrar só esta máquina no painel, em vez de esperar ao vento pela
    /// amostra de um par antigo.
    #[must_use]
    pub fn can_watch_peer_usage(&self) -> bool {
        self.peer_understands(version::supports_usage)
    }

    /// Peça ao par que comece, ou pare, de mandar a amostra dele.
    pub(super) fn watch_peer_usage(&mut self, now: Timestamp, watch: bool, out: &mut CommandBatch) {
        if self.can_watch_peer_usage() {
            self.send(now, Message::Control(Control::WatchUsage { watch }), out);
        }
    }

    /// Mande ao par a amostra desta máquina, que ele pediu.
    pub(super) fn share_usage(
        &mut self,
        now: Timestamp,
        sample: UsageSample,
        out: &mut CommandBatch,
    ) {
        if self.can_watch_peer_usage() {
            self.send(now, Message::Control(Control::Usage(sample)), out);
        }
    }

    /// O par mandou a amostra dele. Ela vem de outra máquina: entra nos tetos antes de seguir.
    pub(super) fn on_peer_usage(sample: UsageSample, out: &mut CommandBatch) {
        out.push(Command::Notify(Notice::PeerUsage(sample.bounded())));
    }

    /// Se o quadro é um `Pong`, anota a volta pelo portador que o trouxe.
    ///
    /// Chamada antes da detecção de repetição, que descartaria a cópia do portador mais lento.
    pub(super) fn measure_route(
        now: Timestamp,
        carrier: Carrier,
        frame: &Frame,
        out: &mut CommandBatch,
    ) {
        let Message::Control(Control::Pong { stamp_micros }) = frame.message else {
            return;
        };
        let sent = Timestamp::from_micros(stamp_micros);
        if sent > now {
            return; // carimbo do futuro: só pode ser de outra sessão
        }
        out.push(Command::Notify(Notice::RouteLatency {
            carrier,
            rtt: now.since(sent),
        }));
    }
}
